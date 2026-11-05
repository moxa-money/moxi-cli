use anyhow::{bail, Context, Result};
use base64::Engine;
use chrono::{DateTime, Local, NaiveDate};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use fs2::FileExt;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::model::{
    check_agent, check_body, check_desk, check_op, check_op_replay, hash_file, known_op, now_utc,
    Action, DeskNeed, Event, Hit, Message, Op, OwnedRecord, ReadyPlan, Record, Show, NOTICE_BOARD,
    OPERATOR, SCHEMA_VERSION,
};
use crate::owners::{self, owner};

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    pub fn init(&self) -> Result<PathBuf> {
        std::fs::create_dir_all(self.data_dir())?;
        let log = self.log_path();
        if !log.exists() {
            File::create(&log)?;
        }
        self.with_write(|conn| sync_conn(conn, &self.log_path()))?;
        Ok(self.data_dir())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn plan_open(
        &self,
        plan_ref: String,
        path: PathBuf,
        repo: String,
        depends_on: Vec<String>,
        summary: String,
        tags: Vec<String>,
        entities: Vec<String>,
    ) -> Result<Event> {
        self.require_owner(&repo)?;
        let path = canonicalize_existing(&path)?;
        let content_hash = hash_file(&path)?;
        self.append(Op::PlanOpen {
            plan_ref,
            path: path.display().to_string(),
            content_hash,
            repo,
            depends_on,
            summary,
            tags,
            entities,
        })
    }

    pub fn plan_update(&self, op: Op) -> Result<Event> {
        self.append(op)
    }

    pub fn item_open(&self, op: Op) -> Result<Event> {
        if let Op::ItemOpen { repo, .. } = &op {
            self.require_owner(repo)?;
        }
        self.append(op)
    }

    pub fn act(&self, op: Op) -> Result<Event> {
        if let Op::Action {
            record_id,
            status: Some(status),
            ..
        } = &op
        {
            if status == "closed" || status == "cancelled" {
                if status == "cancelled" {
                    let current = self.show(&record_id.to_string())?;
                    if current.record.status == "closed" {
                        bail!("#{record_id} is closed, which means it was finished. Cancelled is for work that will not be finished.");
                    }
                }
                let waiting = self.unsettled_blocks(*record_id)?;
                if !waiting.is_empty() {
                    bail!("{}", settle_message(&waiting));
                }
            }
        }
        self.append(op)
    }

    /// Records still blocked by this plan that have no later note or unblock naming it.
    fn unsettled_blocks(&self, id: u64) -> Result<Vec<Record>> {
        self.with_read(|conn| {
            let record = find_record(conn, &id.to_string())?;
            waiting_blocks(conn, &record, false)
        })
    }

    pub fn list(
        &self,
        kind: Option<&str>,
        repo: Option<&str>,
        statuses: &[String],
    ) -> Result<Vec<OwnedRecord>> {
        self.with_read(|conn| {
            let mut sql = String::from(
                "SELECT id, kind, plan_ref, plan_id, repo, path, content_hash, worktree, branch,
                        requesting_agent, requested_agent, summary, tags, entities, status,
                        depends_on, blocked_by, pr_id, merge_sha, merge_at, opened_at
                 FROM records WHERE 1=1",
            );
            let mut values: Vec<String> = Vec::new();
            if let Some(kind) = kind {
                sql.push_str(" AND kind = ?");
                values.push(kind.to_string());
            }
            if let Some(repo) = repo {
                sql.push_str(" AND repo = ?");
                values.push(repo.to_string());
            }
            if !statuses.is_empty() {
                sql.push_str(" AND status IN (");
                sql.push_str(&vec!["?"; statuses.len()].join(","));
                sql.push(')');
                values.extend(statuses.iter().cloned());
            }
            sql.push_str(" ORDER BY id");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), row_record)?;
            let owners = owners::load(&self.root)?;
            let mut out = Vec::new();
            for row in rows {
                let record = row?;
                if record.plan_ref.as_deref() == Some("na") {
                    continue;
                }
                let owner = owner_or_unknown(&owners, &record.repo);
                out.push(OwnedRecord { record, owner });
            }
            Ok(out)
        })
    }

    pub fn ready(&self) -> Result<Vec<ReadyPlan>> {
        self.with_read(|conn| {
            let owners = owners::load(&self.root)?;
            let plans = records_where(conn, "kind = 'plan' ORDER BY id")?;
            let mut closed = std::collections::BTreeSet::new();
            for plan in &plans {
                if plan.status == "closed" {
                    if let Some(plan_ref) = &plan.plan_ref {
                        closed.insert(plan_ref.clone());
                    }
                }
            }
            let mut ready = Vec::new();
            for plan in plans {
                if plan.status != "open" {
                    continue;
                }
                if !plan.depends_on.iter().all(|dep| closed.contains(dep)) {
                    continue;
                }
                let items = open_items(conn, plan.id)?;
                let owner = owner_or_unknown(&owners, &plan.repo);
                let items = items
                    .into_iter()
                    .map(|record| {
                        let owner = owner_or_unknown(&owners, &record.repo);
                        OwnedRecord { record, owner }
                    })
                    .collect();
                ready.push(ReadyPlan {
                    record: plan,
                    owner,
                    items,
                });
            }
            Ok(ready)
        })
    }

    /// The standing record for a notice that is not about a project.
    /// It stays cancelled so it is absent from the work list.
    /// Messages on it share one thread per local day, headed by the first message that day.
    pub fn ensure_na(&self) -> Result<u64> {
        let id = if let Ok(show) = self.show("na") {
            show.record.id
        } else {
            let path = self.root.join("README.md");
            let opened = self.plan_open(
                "na".into(),
                path,
                "ledger".into(),
                Vec::new(),
                "Notices with no project".into(),
                Vec::new(),
                Vec::new(),
            )?;
            self.plan_update(Op::PlanUpdate {
                plan_ref: "na".into(),
                path: None,
                content_hash: None,
                depends_on: None,
                summary: None,
                tags: None,
                entities: None,
                status: Some("cancelled".into()),
            })?;
            opened.id
        };
        self.link_na_days(id)?;
        Ok(id)
    }

    /// A record id, a plan ref, or NA.
    pub fn resolve_on(&self, on: &str) -> Result<u64> {
        let key = on.trim().trim_start_matches('#');
        if key.eq_ignore_ascii_case("NA") {
            return self.ensure_na();
        }
        Ok(self.show(key)?.record.id)
    }

    pub fn show(&self, key: &str) -> Result<Show> {
        self.with_read(|conn| {
            let owners = owners::load(&self.root)?;
            let record = find_record(conn, key)?;
            let items = if record.kind == "plan" {
                records_where(conn, &format!("plan_id = {} ORDER BY id", record.id))?
                    .into_iter()
                    .map(|item| {
                        let owner = owner_or_unknown(&owners, &item.repo);
                        OwnedRecord {
                            record: item,
                            owner,
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let actions = actions_for(conn, record.id)?;
            let owner = owner_or_unknown(&owners, &record.repo);
            Ok(Show {
                record,
                owner,
                items,
                actions,
            })
        })
    }

    pub fn related(&self, key: &str) -> Result<Vec<OwnedRecord>> {
        self.with_read(|conn| {
            let owners = owners::load(&self.root)?;
            let record = find_record(conn, key)?;
            let plan_ref = record
                .plan_ref
                .clone()
                .filter(|_| record.kind == "plan")
                .or_else(|| {
                    record.plan_id.and_then(|id| {
                        conn.query_row(
                            "SELECT plan_ref FROM records WHERE id = ?1",
                            params![id as i64],
                            |row| row.get::<_, Option<String>>(0),
                        )
                        .ok()
                        .flatten()
                    })
                });
            let Some(plan_ref) = plan_ref else {
                return Ok(vec![OwnedRecord {
                    owner: owner_or_unknown(&owners, &record.repo),
                    record,
                }]);
            };
            let mut stmt = conn.prepare(
                "SELECT id, kind, plan_ref, plan_id, repo, path, content_hash, worktree, branch,
                        requesting_agent, requested_agent, summary, tags, entities, status,
                        depends_on, blocked_by, pr_id, merge_sha, merge_at, opened_at
                 FROM records
                 WHERE plan_ref = ?1
                    OR blocked_by = ?1
                    OR EXISTS (SELECT 1 FROM json_each(depends_on) WHERE value = ?1)
                    OR id IN (
                        SELECT record_id FROM actions
                        WHERE EXISTS (SELECT 1 FROM json_each(mentions) WHERE value = ?1)
                    )
                 ORDER BY id",
            )?;
            let rows = stmt.query_map(params![plan_ref], row_record)?;
            let mut out = Vec::new();
            for row in rows {
                let record = row?;
                let owner = owner_or_unknown(&owners, &record.repo);
                out.push(OwnedRecord { record, owner });
            }
            Ok(out)
        })
    }

    pub fn search(&self, query: &str) -> Result<Vec<Hit>> {
        let Some(match_query) = fts_query(query) else {
            bail!("search needs a word");
        };
        self.with_read(|conn| {
            let owners = owners::load(&self.root)?;
            let mut stmt = conn.prepare(
                "SELECT fts.rowid, fts.record_id, snippet(fts, 0, '', '', '…', 12)
                 FROM fts WHERE fts MATCH ?1 ORDER BY rank LIMIT 30",
            )?;
            let rows = stmt.query_map(params![match_query], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            let mut hits = Vec::new();
            for row in rows {
                let (rowid, record_id, snippet) = row?;
                let record = match conn.query_row(
                    "SELECT id, kind, plan_ref, plan_id, repo, path, content_hash, worktree, branch,
                            requesting_agent, requested_agent, summary, tags, entities, status,
                            depends_on, blocked_by, pr_id, merge_sha, merge_at, opened_at
                     FROM records WHERE id = ?1",
                    params![record_id],
                    row_record,
                ) {
                    Ok(record) => record,
                    Err(rusqlite::Error::QueryReturnedNoRows) => continue,
                    Err(err) => return Err(err.into()),
                };
                let kind = if rowid == record_id {
                    record.kind.clone()
                } else {
                    conn.query_row(
                        "SELECT kind FROM actions WHERE id = ?1",
                        params![rowid],
                        |row| row.get::<_, String>(0),
                    )
                    .unwrap_or_else(|_| "action".into())
                };
                hits.push(Hit {
                    id: rowid as u64,
                    record_id: record.id,
                    plan_ref: record.plan_ref,
                    kind,
                    owner: owner_or_unknown(&owners, &record.repo),
                    snippet,
                });
            }
            Ok(hits)
        })
    }

    pub fn rebuild(&self) -> Result<()> {
        let _lock = self.lock()?;
        for suffix in ["", "-wal", "-shm"] {
            let path = self.db_path_with(suffix);
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
        let conn = self.open_conn()?;
        sync_conn(&conn, &self.log_path())?;
        Ok(())
    }

    /// Cancel a plan or item, and cancel the plan's unfinished items with the same link.
    /// The link is the agent, the reason, and the pivot plan ref or action id.
    pub fn supersede(
        &self,
        id: u64,
        agent: String,
        pivot: String,
        body: String,
        mentions: Vec<String>,
    ) -> Result<Vec<Event>> {
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        let record = find_record(&conn, &id.to_string())?;
        if record.status == "closed" {
            bail!("#{id} is closed, which means it was finished. Cancelled is for work that will not be finished.");
        }
        if record.status == "cancelled" {
            bail!("#{id} is already cancelled");
        }
        validate_pivot(&conn, &pivot, record.plan_ref.as_deref())?;
        if record.kind == "plan" {
            let waiting = waiting_blocks(&conn, &record, true)?;
            if !waiting.is_empty() {
                bail!("{}", settle_message(&waiting));
            }
        }
        let mut targets = vec![record.id];
        if record.kind == "plan" {
            let mut stmt = conn.prepare(
                "SELECT id FROM records
                 WHERE plan_id = ?1 AND status IN ('open', 'blocked', 'pinned')
                 ORDER BY id",
            )?;
            let rows = stmt.query_map(params![record.id as i64], |row| row.get::<_, i64>(0))?;
            for row in rows {
                targets.push(row? as u64);
            }
        }
        let mut events = Vec::with_capacity(targets.len());
        for target in targets {
            events.push(write_event(
                &conn,
                &log,
                cancel_op(
                    target,
                    agent.clone(),
                    pivot.clone(),
                    body.clone(),
                    mentions.clone(),
                ),
            )?);
        }
        Ok(events)
    }

    /// Set who acts next and send that agent a need. Open needs addressed to the sender on this record are answered.
    pub fn handoff(
        &self,
        id: u64,
        agent: String,
        requested: String,
        parent: Option<u64>,
        body: String,
        mentions: Vec<String>,
    ) -> Result<Vec<Event>> {
        crate::model::check_agent("agent", &agent)?;
        crate::model::check_agent("recipient", &requested)?;
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        find_record(&conn, &id.to_string())?;
        let mut events = Vec::new();
        for notice in open_needs_for(&conn, id, &agent)? {
            events.push(write_event(
                &conn,
                &log,
                ack_op(
                    id,
                    notice,
                    agent.clone(),
                    "did",
                    format!("handed to {requested}"),
                    vec![notice],
                    DeskNeed::default(),
                ),
            )?);
        }
        let handoff = write_event(
            &conn,
            &log,
            Op::Action {
                record_id: id,
                parent_id: parent,
                kind: "handoff".into(),
                body: body.clone(),
                mentions: mentions.clone(),
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: Some(requested.clone()),
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some(agent.clone()),
                pivot: None,
                to: Some(requested.clone()),
                outcome: None,
                thread: merge_thread(&[], parent, &[]),
            
                desk: DeskNeed::default(),
},
        )?;
        let handoff_id = handoff.id;
        events.push(handoff);
        events.push(write_event(
            &conn,
            &log,
            Op::Action {
                record_id: id,
                parent_id: Some(handoff_id),
                kind: "need".into(),
                body,
                mentions,
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some(agent),
                pivot: None,
                to: Some(requested),
                outcome: None,
                thread: vec![handoff_id],
            
                desk: DeskNeed::default(),
},
        )?);
        Ok(events)
    }

    /// Send a need or a tell. `--all` writes one notice to each agent with open
    /// or blocked work on that plan, except the sender and FOUNDER.
    pub fn tell(
        &self,
        id: u64,
        agent: String,
        to: Option<String>,
        all: bool,
        kind: String,
        body: String,
        mentions: Vec<String>,
        thread: Vec<u64>,
        desk: DeskNeed,
    ) -> Result<Vec<Event>> {
        check_agent("agent", &agent)?;
        if kind != "need" && kind != "tell" {
            bail!("tell sends a need or a tell");
        }
        if all == to.is_some() {
            bail!("tell takes --all or --to");
        }
        if !all {
            let recipient = to.clone().unwrap_or_default();
            check_agent("recipient", &recipient)?;
            if recipient == agent {
                bail!("tell does not address the sender");
            }
        }
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        let record = find_record(&conn, &id.to_string())?;
        let thread = merge_thread(&[], None, &thread);
        require_messages(&conn, &thread)?;
        if desk.ping || desk.for_founder {
            if kind != "need" {
                bail!("--for-founder and --ping are needs");
            }
        }
        if desk.ping {
            if all || to.as_deref() != Some("FOUNDER") {
                bail!("--ping is a need to FOUNDER");
            }
            let orch = self.orchestrator_of(&record)?;
            if agent != orch {
                bail!("--ping is only for the orchestrator");
            }
            require_messages(&conn, &desk.via)?;
        }
        if desk.for_founder {
            if all {
                bail!("--for-founder names the orchestrator");
            }
            let orch = self.orchestrator_of(&record)?;
            if to.as_deref() != Some(orch.as_str()) {
                bail!("--for-founder goes to the orchestrator");
            }
        }
        let recipients: Vec<String> = if all {
            plan_assignees(&conn, &record, &agent)?
        } else {
            vec![to.unwrap_or_default()]
        };
        if recipients.is_empty() {
            bail!("--all has no agent with open work on this plan. Use --to AGENT");
        }
        let mut events = Vec::new();
        for recipient in recipients {
            events.push(write_event(
                &conn,
                &log,
                Op::Action {
                    record_id: id,
                    parent_id: None,
                    kind: kind.clone(),
                    body: body.clone(),
                    mentions: mentions.clone(),
                    status: None,
                    blocked_by: None,
                    clear_blocked_by: false,
                    requested_agent: None,
                    pr_id: None,
                    merge_sha: None,
                    merge_at: None,
                    worktree: None,
                    branch: None,
                    clear_worktree: false,
                    agent: Some(agent.clone()),
                    pivot: None,
                    to: Some(recipient),
                    outcome: None,
                    thread: thread.clone(),
                    desk: desk.clone(),
                },
            )?);
        }
        Ok(events)
    }

    /// Post to the channel. It is not inbox mail and it takes no acknowledgement.
    pub fn notice(
        &self,
        id: u64,
        agent: String,
        to: Option<String>,
        body: String,
        mentions: Vec<String>,
        thread: Vec<u64>,
    ) -> Result<Vec<Event>> {
        check_agent("agent", &agent)?;
        if let Some(recipient) = &to {
            if recipient != NOTICE_BOARD {
                bail!("a notice is addressed to {NOTICE_BOARD}");
            }
        }
        let to = Some(NOTICE_BOARD.to_string());
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        let _record = find_record(&conn, &id.to_string())?;
        let thread = merge_thread(&[], None, &thread);
        require_messages(&conn, &thread)?;
        let event = write_event(
            &conn,
            &log,
            Op::Action {
                record_id: id,
                parent_id: None,
                kind: "notice".into(),
                body,
                mentions,
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some(agent),
                pivot: None,
                to,
                outcome: None,
                thread,
            
                desk: DeskNeed::default(),
},
        )?;
        Ok(vec![event])
    }

    /// Record which existing messages this message addresses. It does not send mail.
    pub fn link_thread(&self, message_id: u64, agent: String, thread: Vec<u64>) -> Result<Event> {
        check_agent("agent", &agent)?;
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        let _ = action_by_id(&conn, message_id)?;
        require_messages(&conn, &thread)?;
        write_event(
            &conn,
            &log,
            Op::Thread {
                message_id,
                thread,
                agent,
            },
        )
    }

    /// Answer one notice. Only the recipient may answer.
    pub fn ack(
        &self,
        notice_id: u64,
        agent: String,
        outcome: String,
        body: String,
        thread: Vec<u64>,
    ) -> Result<Vec<Event>> {
        crate::model::check_agent("agent", &agent)?;
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        let notice = action_by_id(&conn, notice_id)?;
        if notice.kind == "notice" {
            bail!("#{notice_id} is a channel post. Nothing to acknowledge");
        }
        if notice.kind != "need" && notice.kind != "tell" {
            bail!("#{notice_id} is not a notice");
        }
        if notice.to.as_deref() != Some(agent.as_str()) {
            bail!(
                "#{notice_id} is addressed to {}, not {agent}",
                notice.to.unwrap_or_else(|| "no one".into())
            );
        }
        if notice_answered(&conn, notice_id)? {
            bail!("#{notice_id} is already answered");
        }
        if outcome == "ackn" && notice.kind != "tell" {
            bail!("#{notice_id} is a need. Reply, or mark it did");
        }
        if outcome == "ackn" && is_answer_tell(&conn, &notice)? {
            bail!(
                "#{notice_id} is a reply to a request, not a fact. done closes it. reply TEXT sends work back"
            );
        }
        let thread = merge_thread(&[], Some(notice_id), &thread);
        require_messages(&conn, &thread)?;
        let mut events = vec![write_event(
            &conn,
            &log,
            ack_op(
                notice.record_id,
                notice_id,
                agent.clone(),
                &outcome,
                body.clone(),
                thread,
                DeskNeed::default(),
            ),
        )?];
        if outcome == "ask-founder" {
            events.push(write_event(
                &conn,
                &log,
                Op::Action {
                    record_id: notice.record_id,
                    parent_id: Some(notice_id),
                    kind: "need".into(),
                    body,
                    mentions: vec![],
                    status: None,
                    blocked_by: None,
                    clear_blocked_by: false,
                    requested_agent: None,
                    pr_id: None,
                    merge_sha: None,
                    merge_at: None,
                    worktree: None,
                    branch: None,
                    clear_worktree: false,
                    agent: Some(agent),
                    pivot: None,
                    to: Some("FOUNDER".into()),
                    outcome: None,
                    thread: vec![notice_id],
                
                    desk: DeskNeed::default(),
},
            )?);
        }
        Ok(events)
    }

    /// Answer a notice and send that text back to its sender.
    pub fn reply(
        &self,
        notice_id: u64,
        agent: String,
        body: String,
        thread: Vec<u64>,
    ) -> Result<Vec<Event>> {
        check_agent("agent", &agent)?;
        check_body(&body)?;
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        let notice = action_by_id(&conn, notice_id)?;
        if notice.kind == "notice" {
            bail!("#{notice_id} is a channel post. Nothing to acknowledge");
        }
        if notice.kind != "need" && notice.kind != "tell" {
            bail!("#{notice_id} is not a notice");
        }
        if notice.to.as_deref() != Some(agent.as_str()) {
            bail!(
                "#{notice_id} is addressed to {}, not {agent}",
                notice.to.unwrap_or_else(|| "no one".into())
            );
        }
        if notice_answered(&conn, notice_id)? {
            bail!("#{notice_id} is already answered");
        }
        let thread = merge_thread(&[], Some(notice_id), &thread);
        require_messages(&conn, &thread)?;
        let mut events = vec![write_event(
            &conn,
            &log,
            ack_op(
                notice.record_id,
                notice_id,
                agent.clone(),
                "answered",
                body.clone(),
                thread.clone(),
                DeskNeed::default(),
            ),
        )?];
        if let Some(sender) = notice.agent.clone() {
            if sender != agent {
                check_agent("recipient", &sender)?;
                events.push(write_event(
                    &conn,
                    &log,
                    Op::Action {
                        record_id: notice.record_id,
                        parent_id: Some(notice_id),
                        kind: "tell".into(),
                        body,
                        mentions: vec![],
                        status: None,
                        blocked_by: None,
                        clear_blocked_by: false,
                        requested_agent: None,
                        pr_id: None,
                        merge_sha: None,
                        merge_at: None,
                        worktree: None,
                        branch: None,
                        clear_worktree: false,
                        agent: Some(agent),
                        pivot: None,
                        to: Some(sender),
                        outcome: Some("answered".into()),
                        thread: thread.clone(),
                    
                        desk: DeskNeed::default(),
},
                )?);
            }
        }
        Ok(events)
    }

    /// The sender cancels their own open need or tell.
    pub fn withdraw(&self, notice_id: u64, agent: String, body: String) -> Result<Event> {
        check_agent("agent", &agent)?;
        check_body(&body)?;
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        let notice = action_by_id(&conn, notice_id)?;
        if notice.kind != "need" && notice.kind != "tell" {
            bail!("#{notice_id} is not a need or a tell");
        }
        let Some(sender) = notice.agent.as_deref() else {
            bail!("#{notice_id} has no sender");
        };
        if sender != agent {
            bail!("#{notice_id} is from {sender}, not {agent}");
        }
        if is_answer_tell(&conn, &notice)? {
            bail!("an answer tell cannot be withdrawn");
        }
        if signed_answer(&conn, notice_id)? {
            bail!("a signed answer cannot be withdrawn");
        }
        if !notice_open(&conn, notice_id)? {
            bail!("#{notice_id} is not open");
        }
        write_event(
            &conn,
            &log,
            ack_op(
                notice.record_id,
                notice_id,
                agent,
                "withdrawn",
                body,
                vec![notice_id],
                DeskNeed::default(),
            ),
        )
    }

    /// One ledger event for a desk answer. The kind is the key's event, and nothing else is written.
    pub fn answer(
        &self,
        notice_id: u64,
        agent: String,
        kind: String,
        body: String,
        to: Option<String>,
        outcome: Option<String>,
        thread: Vec<u64>,
        signature: Option<String>,
        signer: Option<String>,
        public_key: Option<String>,
    ) -> Result<Event> {
        check_agent("agent", &agent)?;
        check_body(&body)?;
        if !matches!(kind.as_str(), "ack" | "tell" | "need") {
            bail!("a desk answer is an ack, a tell, or a need");
        }
        if outcome.as_deref() == Some("moved") && kind != "tell" {
            bail!("moved is a reply");
        }
        if let Some(value) = outcome.as_deref() {
            if !crate::model::known_outcome(value) {
                bail!("unknown outcome");
            }
        }
        if kind == "ack" && !matches!(outcome.as_deref(), Some("did" | "done")) {
            bail!("a desk ack is done");
        }
        if kind == "tell" {
            if let Some(value) = outcome.as_deref() {
                if value != "moved" && value != "answered" {
                    bail!("a desk reply outcome is answered");
                }
            }
        }
        if kind != "ack" && to.is_none() {
            bail!("a desk {kind} names who receives it");
        }
        let desk = answer_desk(&agent, signature, signer)?;
        if agent == "FOUNDER" {
            verify_founder_signature(
                &self.root,
                notice_id,
                &kind,
                &body,
                to.as_deref().unwrap_or(""),
                outcome.as_deref().unwrap_or(""),
                desk.signature.as_deref().unwrap_or(""),
                desk.signer.as_deref().unwrap_or(""),
                public_key.as_deref().unwrap_or(""),
            )?;
        }
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        let notice = action_by_id(&conn, notice_id)?;
        if notice.kind != "need" && notice.kind != "tell" {
            bail!("#{notice_id} is not a request");
        }
        if notice.to.as_deref() != Some(agent.as_str()) {
            bail!(
                "#{notice_id} is addressed to {}, not {agent}",
                notice.to.unwrap_or_else(|| "no one".into())
            );
        }
        if notice_answered(&conn, notice_id)? {
            bail!("#{notice_id} is already answered");
        }
        let thread = merge_thread(&[], Some(notice_id), &thread);
        require_messages(&conn, &thread)?;
        if notice.kind == "tell" {
            if kind == "ack" {
                if !is_answer_tell(&conn, &notice)? {
                    bail!("#{notice_id} is a fact. ackn closes it. reply TEXT sends work back");
                }
                return write_event(
                    &conn,
                    &log,
                    ack_op(
                        notice.record_id,
                        notice_id,
                        agent,
                        outcome.as_deref().unwrap_or("did"),
                        body,
                        thread,
                        desk,
                    ),
                );
            }
            if kind != "need" || notice.to.as_deref() != Some("FOUNDER") {
                bail!("#{notice_id} is a tell to FOUNDER. Reply writes a signed need");
            }
            let need = write_event(
                &conn,
                &log,
                Op::Action {
                    record_id: notice.record_id,
                    parent_id: Some(notice_id),
                    kind: "need".into(),
                    body: body.clone(),
                    mentions: vec![],
                    status: None,
                    blocked_by: None,
                    clear_blocked_by: false,
                    requested_agent: None,
                    pr_id: None,
                    merge_sha: None,
                    merge_at: None,
                    worktree: None,
                    branch: None,
                    clear_worktree: false,
                    agent: Some(agent.clone()),
                    pivot: None,
                    to: to.clone(),
                    outcome: Some("answered".into()),
                    thread: thread.clone(),
                    desk: desk.clone(),
                },
            )?;
            write_event(
                &conn,
                &log,
                ack_op(
                    notice.record_id,
                    notice_id,
                    agent,
                    "answered",
                    body,
                    thread,
                    DeskNeed::default(),
                ),
            )?;
            return Ok(need);
        }
        if notice.kind != "need" {
            bail!("#{notice_id} is not a request");
        }
        let op = if kind == "ack" {
            ack_op(notice.record_id, notice_id, agent, "did", body, thread, desk)
        } else {
            let stored = match outcome.as_deref() {
                Some("moved") => Some("moved".to_string()),
                Some("answered") => Some("answered".to_string()),
                _ => None,
            };
            Op::Action {
                record_id: notice.record_id,
                parent_id: Some(notice_id),
                kind,
                body,
                mentions: vec![],
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some(agent),
                pivot: None,
                to,
                outcome: stored,
                thread,
                desk,
            }
        };
        write_event(&conn, &log, op)
    }

    /// Flat list of notices. No sender, recipient, or record returns every notice.
    pub fn messages(
        &self,
        from: Option<&str>,
        to: Option<&str>,
        record: Option<&str>,
        all: bool,
    ) -> Result<Vec<Message>> {
        self.associate_na_query(record)?;
        self.with_read(|conn| list_messages(conn, from, to, record, all, false))
    }

    /// A query for the standing NA record links each older message to that local day's first message.
    fn associate_na_query(&self, record: Option<&str>) -> Result<()> {
        let Some(key) = record.map(str::trim).filter(|key| !key.is_empty()) else {
            return Ok(());
        };
        let id = match self.show("na") {
            Ok(show) => show.record.id,
            Err(_) => return Ok(()),
        };
        if !key.eq_ignore_ascii_case("na") && key != id.to_string() {
            return Ok(());
        }
        self.link_na_days(id)
    }

    /// Each local day of NA has one thread. The first message that day is the head.
    fn link_na_days(&self, record_id: u64) -> Result<()> {
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        let actions = actions_for(&conn, record_id)?;
        let mut heads = std::collections::BTreeMap::new();
        for action in &actions {
            if let Some(day) = local_day(&action.at) {
                heads.entry(day).or_insert(action.id);
            }
        }
        for action in &actions {
            let Some(day) = local_day(&action.at) else {
                continue;
            };
            let Some(head) = heads.get(&day).copied() else {
                continue;
            };
            if head == action.id || action.thread.contains(&head) {
                continue;
            }
            let agent = action
                .agent
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "LEAD".into());
            write_event(
                &conn,
                &log,
                Op::Thread {
                    message_id: action.id,
                    thread: vec![head],
                    agent,
                },
            )?;
        }
        Ok(())
    }

    /// Open notices plus the newest channel posts. `limit` is how many posts to keep.
    pub fn channel_messages(
        &self,
        from: Option<&str>,
        to: Option<&str>,
        record: Option<&str>,
        all: bool,
        limit: u64,
    ) -> Result<Vec<Message>> {
        self.with_read(|conn| {
            let rows = list_messages(conn, from, to, record, all, true)?;
            if all {
                return Ok(rows);
            }
            Ok(trim_notices(rows, limit))
        })
    }

    fn append(&self, op: Op) -> Result<Event> {
        check_op(&op)?;
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        let log = self.log_path();
        sync_conn(&conn, &log)?;
        let event = write_event(&conn, &log, op.clone())?;
        self.wake_ready(&conn, &log, &op)?;
        Ok(event)
    }

    /// A need to the assignee when work is unblocked or a plan it waits on closes.
    /// The actor passes --agent. No notice is written when the actor is the assignee.
    fn wake_ready(&self, conn: &Connection, log: &Path, op: &Op) -> Result<()> {
        let Op::Action {
            record_id,
            kind,
            agent,
            ..
        } = op
        else {
            return Ok(());
        };
        let Some(sender) = agent.clone().filter(|name| !name.is_empty()) else {
            return Ok(());
        };
        if kind != "unblock" && kind != "close" && kind != "merge" {
            return Ok(());
        }
        let record = find_record(conn, &record_id.to_string())?;
        if kind == "unblock" {
            return self.notify_assignee(
                conn,
                log,
                &sender,
                &record,
                &format!("#{} is unblocked. It is yours.", record.id),
            );
        }
        if record.kind != "plan" {
            return Ok(());
        }
        let Some(plan_ref) = record.plan_ref.clone() else {
            return Ok(());
        };
        let others = records_where(conn, "status NOT IN ('closed', 'cancelled') ORDER BY id")?;
        for other in others {
            if other.id == record.id {
                continue;
            }
            let blocked = other.blocked_by.as_deref() == Some(plan_ref.as_str());
            let depends = other.depends_on.iter().any(|dep| dep == &plan_ref);
            if !blocked && !depends {
                continue;
            }
            let word = if blocked { "unblocked" } else { "ready" };
            self.notify_assignee(
                conn,
                log,
                &sender,
                &other,
                &format!("{plan_ref} is closed. #{} is {word}. It is yours.", other.id),
            )?;
        }
        Ok(())
    }

    fn notify_assignee(
        &self,
        conn: &Connection,
        log: &Path,
        sender: &str,
        record: &Record,
        body: &str,
    ) -> Result<()> {
        let Some(recipient) = self.assignee(record)? else {
            return Ok(());
        };
        if recipient == sender {
            return Ok(());
        }
        if open_need_to(conn, record.id, &recipient)? {
            return Ok(());
        }
        write_event(
            conn,
            log,
            Op::Action {
                record_id: record.id,
                parent_id: None,
                kind: "need".into(),
                body: body.to_string(),
                mentions: vec![],
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some(sender.to_string()),
                pivot: None,
                to: Some(recipient),
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
},
        )?;
        Ok(())
    }

    /// The plan's repo owner. An item with no plan uses its assignee.
    fn orchestrator_of(&self, record: &Record) -> Result<String> {
        if record.kind != "plan" {
            if let Some(name) = &record.requested_agent {
                if !name.is_empty() {
                    return Ok(name.clone());
                }
            }
        }
        let owners = owners::load(&self.root)?;
        owner(&owners, &record.repo).map(str::to_string)
    }

    fn assignee(&self, record: &Record) -> Result<Option<String>> {
        if let Some(name) = &record.requested_agent {
            if !name.is_empty() {
                return Ok(Some(name.clone()));
            }
        }
        let owners = owners::load(self.root())?;
        match owner(&owners, &record.repo) {
            Ok(name) => Ok(Some(name.to_string())),
            _ => Ok(None),
        }
    }

    fn require_owner(&self, repo: &str) -> Result<()> {
        let owners = owners::load(&self.root)?;
        owner(&owners, repo).map(|_| ())
    }

    fn with_read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        sync_conn(&conn, &self.log_path())?;
        f(&conn)
    }

    fn with_write<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let _lock = self.lock()?;
        let conn = self.open_conn()?;
        f(&conn)
    }

    fn lock(&self) -> Result<Lock> {
        std::fs::create_dir_all(self.data_dir())?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.data_dir().join("comms.lock"))?;
        file.lock_exclusive()?;
        Ok(Lock { file })
    }

    fn open_conn(&self) -> Result<Connection> {
        std::fs::create_dir_all(self.data_dir())?;
        let conn = Connection::open(self.db_path())?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&conn)?;
        Ok(conn)
    }

    fn log_path(&self) -> PathBuf {
        self.data_dir().join("events.jsonl")
    }

    fn db_path(&self) -> PathBuf {
        self.data_dir().join("comms.db")
    }

    fn db_path_with(&self, suffix: &str) -> PathBuf {
        self.data_dir().join(format!("comms.db{suffix}"))
    }
}

struct Lock {
    file: File,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS records (
            id INTEGER PRIMARY KEY,
            kind TEXT NOT NULL,
            plan_ref TEXT,
            plan_id INTEGER,
            repo TEXT NOT NULL,
            path TEXT,
            content_hash TEXT,
            worktree TEXT,
            branch TEXT,
            requesting_agent TEXT,
            requested_agent TEXT,
            summary TEXT NOT NULL,
            tags TEXT NOT NULL,
            entities TEXT NOT NULL,
            status TEXT NOT NULL,
            depends_on TEXT NOT NULL,
            blocked_by TEXT,
            pr_id TEXT,
            merge_sha TEXT,
            merge_at TEXT,
            opened_at TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS records_plan_ref
            ON records(plan_ref) WHERE kind = 'plan';
        CREATE INDEX IF NOT EXISTS records_plan_id ON records(plan_id);
        CREATE TABLE IF NOT EXISTS actions (
            id INTEGER PRIMARY KEY,
            record_id INTEGER NOT NULL,
            parent_id INTEGER,
            at TEXT NOT NULL,
            kind TEXT NOT NULL,
            body TEXT NOT NULL,
            mentions TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS actions_record ON actions(record_id);
        CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
            text,
            record_id UNINDEXED,
            tokenize = 'porter unicode61'
        );",
    )?;
    ensure_column(conn, "actions", "agent", "TEXT")?;
    ensure_column(conn, "actions", "pivot", "TEXT")?;
    ensure_column(conn, "actions", "to_agent", "TEXT")?;
    ensure_column(conn, "actions", "outcome", "TEXT")?;
    ensure_column(conn, "actions", "thread", "TEXT")?;
    ensure_column(conn, "actions", "desk", "TEXT")?;
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, decl: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
        [],
    )?;
    Ok(())
}

fn sync_conn(conn: &Connection, log: &Path) -> Result<()> {
    if !log.exists() {
        return Ok(());
    }
    let events = read_log(log)?;
    let last_id = meta(conn, "last_id")?.unwrap_or_else(|| "0".into());
    let last_id: u64 = last_id.parse().context("meta last_id")?;
    let max_id = events.last().map(|event| event.id).unwrap_or(0);
    if max_id < last_id {
        bail!("log rewound behind the projection; run `comms rebuild`");
    }
    let pending: Vec<&Event> = events.iter().filter(|event| event.id > last_id).collect();
    let mut expect = last_id + 1;
    for event in &pending {
        if event.id != expect {
            bail!("log id gap at {expect}");
        }
        expect += 1;
    }
    let tx = conn.unchecked_transaction()?;
    for event in pending {
        apply_event(&tx, event)?;
        set_meta(&tx, "last_id", &event.id.to_string())?;
    }
    tx.commit()?;
    Ok(())
}

fn apply_event(conn: &Connection, event: &Event) -> Result<()> {
    check_op_replay(&event.op)?;
    match &event.op {
        Op::PlanOpen {
            plan_ref,
            path,
            content_hash,
            repo,
            depends_on,
            summary,
            tags,
            entities,
        } => {
            if plan_id(conn, plan_ref)?.is_some() {
                bail!("plan {plan_ref} already exists");
            }
            insert_record(
                conn,
                event,
                "plan",
                Some(plan_ref),
                None,
                repo,
                Some(path),
                Some(content_hash),
                None,
                None,
                None,
                None,
                summary,
                tags,
                entities,
                "open",
                depends_on,
            )?;
        }
        Op::PlanUpdate {
            plan_ref,
            path,
            content_hash,
            depends_on,
            summary,
            tags,
            entities,
            status,
        } => {
            let id =
                plan_id(conn, plan_ref)?.with_context(|| format!("unknown plan {plan_ref}"))?;
            if let Some(path) = path {
                conn.execute(
                    "UPDATE records SET path = ?1 WHERE id = ?2",
                    params![path, id as i64],
                )?;
            }
            if let Some(content_hash) = content_hash {
                conn.execute(
                    "UPDATE records SET content_hash = ?1 WHERE id = ?2",
                    params![content_hash, id as i64],
                )?;
            }
            if let Some(depends_on) = depends_on {
                conn.execute(
                    "UPDATE records SET depends_on = ?1 WHERE id = ?2",
                    params![json(depends_on)?, id as i64],
                )?;
            }
            if let Some(summary) = summary {
                conn.execute(
                    "UPDATE records SET summary = ?1 WHERE id = ?2",
                    params![summary, id as i64],
                )?;
            }
            if let Some(tags) = tags {
                conn.execute(
                    "UPDATE records SET tags = ?1 WHERE id = ?2",
                    params![json(tags)?, id as i64],
                )?;
            }
            if let Some(entities) = entities {
                conn.execute(
                    "UPDATE records SET entities = ?1 WHERE id = ?2",
                    params![json(entities)?, id as i64],
                )?;
            }
            if let Some(status) = status {
                conn.execute(
                    "UPDATE records SET status = ?1 WHERE id = ?2",
                    params![status, id as i64],
                )?;
            }
            reindex_record(conn, id)?;
        }
        Op::ItemOpen {
            plan_ref,
            repo,
            worktree,
            branch,
            requesting_agent,
            requested_agent,
            summary,
            tags,
            entities,
        } => {
            let plan =
                plan_id(conn, plan_ref)?.with_context(|| format!("unknown plan {plan_ref}"))?;
            insert_record(
                conn,
                event,
                "item",
                Some(plan_ref),
                Some(plan),
                repo,
                None,
                None,
                worktree.as_deref(),
                branch.as_deref(),
                Some(requesting_agent),
                Some(requested_agent),
                summary,
                tags,
                entities,
                "open",
                &[],
            )?;
        }
        Op::Action {
            record_id,
            parent_id,
            kind,
            body,
            mentions,
            status,
            blocked_by,
            clear_blocked_by,
            requested_agent,
            pr_id,
            merge_sha,
            merge_at,
            worktree,
            branch,
            clear_worktree,
            agent,
            pivot,
            to,
            outcome,
            thread,
        
            desk,
} => {
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM records WHERE id = ?1",
                    params![*record_id as i64],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if !exists {
                bail!("unknown record {record_id}");
            }
            if let Some(parent_id) = parent_id {
                let parent_record: u64 = conn
                    .query_row(
                        "SELECT record_id FROM actions WHERE id = ?1",
                        params![*parent_id as i64],
                        |row| row.get::<_, i64>(0),
                    )
                    .with_context(|| format!("unknown parent action {parent_id}"))?
                    as u64;
                if parent_record != *record_id {
                    bail!("parent action {parent_id} is on a different record");
                }
            }
            let stored_thread = merge_thread(&[], *parent_id, thread);
            require_messages(conn, &stored_thread)?;
            conn.execute(
                "INSERT INTO actions (id, record_id, parent_id, at, kind, body, mentions, agent, pivot, to_agent, outcome, thread, desk)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    event.id as i64,
                    *record_id as i64,
                    parent_id.map(|id| id as i64),
                    event.at,
                    kind,
                    body,
                    json(mentions)?,
                    agent,
                    pivot,
                    to.clone().or_else(|| {
                        if kind == "handoff" {
                            requested_agent.clone()
                        } else {
                            None
                        }
                    }),
                    outcome,
                    json(&stored_thread)?,
                    json(desk)?,
                ],
            )?;
            if let Some(status) = status {
                conn.execute(
                    "UPDATE records SET status = ?1 WHERE id = ?2",
                    params![status, *record_id as i64],
                )?;
            }
            if *clear_blocked_by {
                conn.execute(
                    "UPDATE records SET blocked_by = NULL WHERE id = ?1",
                    params![*record_id as i64],
                )?;
            }
            if let Some(blocked_by) = blocked_by {
                conn.execute(
                    "UPDATE records SET blocked_by = ?1 WHERE id = ?2",
                    params![blocked_by, *record_id as i64],
                )?;
            }
            if let Some(requested_agent) = requested_agent {
                conn.execute(
                    "UPDATE records SET requested_agent = ?1 WHERE id = ?2",
                    params![requested_agent, *record_id as i64],
                )?;
            }
            if let Some(pr_id) = pr_id {
                conn.execute(
                    "UPDATE records SET pr_id = ?1 WHERE id = ?2",
                    params![pr_id, *record_id as i64],
                )?;
            }
            if let Some(merge_sha) = merge_sha {
                conn.execute(
                    "UPDATE records SET merge_sha = ?1 WHERE id = ?2",
                    params![merge_sha, *record_id as i64],
                )?;
            }
            if let Some(merge_at) = merge_at {
                conn.execute(
                    "UPDATE records SET merge_at = ?1 WHERE id = ?2",
                    params![merge_at, *record_id as i64],
                )?;
            }
            if *clear_worktree {
                conn.execute(
                    "UPDATE records SET worktree = NULL, branch = NULL WHERE id = ?1",
                    params![*record_id as i64],
                )?;
            }
            if let Some(worktree) = worktree {
                conn.execute(
                    "UPDATE records SET worktree = ?1 WHERE id = ?2",
                    params![worktree, *record_id as i64],
                )?;
            }
            if let Some(branch) = branch {
                conn.execute(
                    "UPDATE records SET branch = ?1 WHERE id = ?2",
                    params![branch, *record_id as i64],
                )?;
            }
            reindex_record(conn, *record_id)?;
            index_row(
                conn,
                event.id,
                *record_id,
                &format!("{kind} {body} {}", mentions.join(" ")),
            )?;
        }
        Op::Thread {
            message_id,
            thread,
            agent: _,
        } => {
            let action = action_by_id(conn, *message_id)?;
            require_messages(conn, thread)?;
            let merged = merge_thread(&action.thread, action.parent_id, thread);
            conn.execute(
                "UPDATE actions SET thread = ?1 WHERE id = ?2",
                params![json(&merged)?, *message_id as i64],
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_record(
    conn: &Connection,
    event: &Event,
    kind: &str,
    plan_ref: Option<&String>,
    plan_id: Option<u64>,
    repo: &str,
    path: Option<&String>,
    content_hash: Option<&String>,
    worktree: Option<&str>,
    branch: Option<&str>,
    requesting_agent: Option<&String>,
    requested_agent: Option<&String>,
    summary: &str,
    tags: &[String],
    entities: &[String],
    status: &str,
    depends_on: &[String],
) -> Result<()> {
    conn.execute(
        "INSERT INTO records (
            id, kind, plan_ref, plan_id, repo, path, content_hash, worktree, branch,
            requesting_agent, requested_agent, summary, tags, entities, status, depends_on, opened_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            event.id as i64,
            kind,
            plan_ref,
            plan_id.map(|id| id as i64),
            repo,
            path,
            content_hash,
            worktree,
            branch,
            requesting_agent,
            requested_agent,
            summary,
            json(tags)?,
            json(entities)?,
            status,
            json(depends_on)?,
            event.at
        ],
    )?;
    reindex_record(conn, event.id)?;
    Ok(())
}

fn reindex_record(conn: &Connection, id: u64) -> Result<()> {
    let record = conn.query_row(
        "SELECT id, kind, plan_ref, plan_id, repo, path, content_hash, worktree, branch,
                requesting_agent, requested_agent, summary, tags, entities, status,
                depends_on, blocked_by, pr_id, merge_sha, merge_at, opened_at
         FROM records WHERE id = ?1",
        params![id as i64],
        row_record,
    )?;
    let text = [
        record.plan_ref.unwrap_or_default(),
        record.summary,
        record.tags.join(" "),
        record.entities.join(" "),
        record.repo,
        record.pr_id.unwrap_or_default(),
        record.blocked_by.unwrap_or_default(),
        record.depends_on.join(" "),
    ]
    .join(" ");
    index_row(conn, id, id, &text)
}

fn index_row(conn: &Connection, rowid: u64, record_id: u64, text: &str) -> Result<()> {
    conn.execute("DELETE FROM fts WHERE rowid = ?1", params![rowid as i64])?;
    conn.execute(
        "INSERT INTO fts(rowid, text, record_id) VALUES (?1, ?2, ?3)",
        params![rowid as i64, text, record_id as i64],
    )?;
    Ok(())
}

fn plan_id(conn: &Connection, plan_ref: &str) -> Result<Option<u64>> {
    match conn.query_row(
        "SELECT id FROM records WHERE kind = 'plan' AND plan_ref = ?1",
        params![plan_ref],
        |row| row.get::<_, i64>(0),
    ) {
        Ok(id) => Ok(Some(id as u64)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Agents assigned to open or blocked work on the plan. The sender and the
/// operator are left out. One name each.
fn plan_assignees(conn: &Connection, record: &Record, sender: &str) -> Result<Vec<String>> {
    let plan_id = if record.kind == "plan" {
        record.id
    } else {
        record.plan_id.unwrap_or(record.id)
    };
    let mut stmt = conn.prepare(
        "SELECT requested_agent FROM records
         WHERE (id = ?1 OR plan_id = ?1)
           AND status IN ('open', 'blocked')
           AND requested_agent IS NOT NULL",
    )?;
    let names = stmt.query_map(params![plan_id as i64], |row| {
        row.get::<_, String>(0)
    })?;
    let mut assigned = BTreeSet::new();
    for name in names {
        assigned.insert(name?);
    }
    Ok(assigned
        .into_iter()
        .filter(|name| name != sender && name != "FOUNDER")
        .collect())
}

fn open_need_to(conn: &Connection, record_id: u64, recipient: &str) -> Result<bool> {
    for action in actions_for(conn, record_id)? {
        if action.kind == "need"
            && action.to.as_deref() == Some(recipient)
            && !notice_answered(conn, action.id)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn find_record(conn: &Connection, key: &str) -> Result<Record> {
    if key != "na" && key.eq_ignore_ascii_case("na") {
        return find_record(conn, "na");
    }
    if let Ok(id) = key.parse::<u64>() {
        if let Ok(record) = record_by_id(conn, id) {
            return Ok(record);
        }
    }
    conn.query_row(
        "SELECT id, kind, plan_ref, plan_id, repo, path, content_hash, worktree, branch,
                requesting_agent, requested_agent, summary, tags, entities, status,
                depends_on, blocked_by, pr_id, merge_sha, merge_at, opened_at
         FROM records WHERE kind = 'plan' AND plan_ref = ?1",
        params![key],
        row_record,
    )
    .with_context(|| format!("no record {key}"))
}

fn record_by_id(conn: &Connection, id: u64) -> Result<Record> {
    conn.query_row(
        "SELECT id, kind, plan_ref, plan_id, repo, path, content_hash, worktree, branch,
                requesting_agent, requested_agent, summary, tags, entities, status,
                depends_on, blocked_by, pr_id, merge_sha, merge_at, opened_at
         FROM records WHERE id = ?1",
        params![id as i64],
        row_record,
    )
    .with_context(|| format!("no record {id}"))
}

fn records_where(conn: &Connection, clause: &str) -> Result<Vec<Record>> {
    let sql = format!(
        "SELECT id, kind, plan_ref, plan_id, repo, path, content_hash, worktree, branch,
                requesting_agent, requested_agent, summary, tags, entities, status,
                depends_on, blocked_by, pr_id, merge_sha, merge_at, opened_at
         FROM records WHERE {clause}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_record)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn open_items(conn: &Connection, plan_id: u64) -> Result<Vec<Record>> {
    records_where(
        conn,
        &format!("plan_id = {plan_id} AND kind = 'item' AND status = 'open' ORDER BY id"),
    )
}

fn waiting_blocks(conn: &Connection, record: &Record, skip_children: bool) -> Result<Vec<Record>> {
    let Some(plan_ref) = record.plan_ref.clone().filter(|_| record.kind == "plan") else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT id, kind, plan_ref, plan_id, repo, path, content_hash, worktree, branch,
                requesting_agent, requested_agent, summary, tags, entities, status,
                depends_on, blocked_by, pr_id, merge_sha, merge_at, opened_at
         FROM records
         WHERE id != ?1 AND status NOT IN ('closed', 'cancelled') AND blocked_by = ?2
         ORDER BY id",
    )?;
    let rows = stmt.query_map(params![record.id as i64, plan_ref], row_record)?;
    let mut waiting = Vec::new();
    for other in rows {
        let other = other?;
        if skip_children && other.plan_id == Some(record.id) {
            continue;
        }
        if !block_settled(conn, other.id, &plan_ref)? {
            waiting.push(other);
        }
    }
    Ok(waiting)
}

fn validate_pivot(conn: &Connection, pivot: &str, own_ref: Option<&str>) -> Result<()> {
    if pivot.chars().all(|ch| ch.is_ascii_digit()) {
        let found: bool = conn
            .query_row(
                "SELECT 1 FROM actions WHERE id = ?1",
                params![pivot.parse::<i64>().unwrap_or(0)],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if !found {
            bail!("no action #{pivot}");
        }
        return Ok(());
    }
    if own_ref == Some(pivot) {
        bail!("a record cannot be superseded by its own plan");
    }
    let replacement = find_record(conn, pivot)?;
    if replacement.kind != "plan" {
        bail!("--by must be a plan ref or an action id");
    }
    Ok(())
}

fn open_needs_for(conn: &Connection, record_id: u64, recipient: &str) -> Result<Vec<u64>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM actions
         WHERE record_id = ?1 AND kind = 'need' AND to_agent = ?2
         ORDER BY id",
    )?;
    let rows = stmt.query_map(params![record_id as i64, recipient], |row| {
        row.get::<_, i64>(0)
    })?;
    let mut open = Vec::new();
    for row in rows {
        let id = row? as u64;
        if !notice_answered(conn, id)? {
            open.push(id);
        }
    }
    Ok(open)
}

fn notice_answered(conn: &Connection, notice_id: u64) -> Result<bool> {
    let found: bool = conn
        .query_row(
            "SELECT 1 FROM actions
             WHERE parent_id = ?1 AND (
                (kind = 'ack' AND outcome IN ('did', 'done', 'ask-founder', 'to_founder', 'ackn', 'answered', 'withdrawn'))
                OR (kind = 'tell' AND outcome IN ('moved', 'answered'))
             )",
            params![notice_id as i64],
            |_| Ok(true),
        )
        .unwrap_or(false);
    Ok(found)
}

fn notice_open(conn: &Connection, notice_id: u64) -> Result<bool> {
    let settled: bool = conn
        .query_row(
            "SELECT 1 FROM actions
             WHERE parent_id = ?1 AND (
                (kind = 'ack' AND outcome IN ('did', 'done', 'later', 'ask-founder', 'to_founder', 'ackn', 'answered', 'withdrawn'))
                OR (kind = 'tell' AND outcome IN ('moved', 'answered'))
             )",
            params![notice_id as i64],
            |_| Ok(true),
        )
        .unwrap_or(false);
    Ok(!settled)
}

fn is_answer_tell(conn: &Connection, notice: &Action) -> Result<bool> {
    if notice.kind != "tell" {
        return Ok(false);
    }
    if matches!(notice.outcome.as_deref(), Some("answered" | "moved")) {
        return Ok(true);
    }
    let Some(parent) = notice.parent_id else {
        return Ok(false);
    };
    let found: bool = conn
        .query_row(
            "SELECT 1 FROM actions
             WHERE parent_id = ?1 AND kind = 'ack' AND outcome IN ('did', 'answered')",
            params![parent as i64],
            |_| Ok(true),
        )
        .unwrap_or(false);
    Ok(found)
}

fn signed_answer(conn: &Connection, notice_id: u64) -> Result<bool> {
    let desk: Option<String> = conn.query_row(
        "SELECT desk FROM actions WHERE id = ?1",
        params![notice_id as i64],
        |row| row.get(0),
    )?;
    Ok(!parse_desk(desk.as_deref())
        .signature
        .unwrap_or_default()
        .is_empty())
}

fn action_by_id(conn: &Connection, id: u64) -> Result<Action> {
    conn.query_row(
        "SELECT id, record_id, parent_id, at, kind, body, mentions, agent, pivot, to_agent, outcome, thread
         FROM actions WHERE id = ?1",
        params![id as i64],
        row_action,
    )
    .with_context(|| format!("unknown action #{id}"))
}

fn recorded_fingerprint(root: &Path) -> Result<String> {
    let path = root.join("signers").join("founder.md");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => bail!("an unsigned answer is a question"),
    };
    parse_recorded(&text)
}

fn parse_recorded(text: &str) -> Result<String> {
    for line in text.lines() {
        if !line.contains("Fingerprint") {
            continue;
        }
        if let Some(start) = line.find('`') {
            let rest = &line[start + 1..];
            if let Some(end) = rest.find('`') {
                let value = &rest[..end];
                if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Ok(value.to_ascii_lowercase());
                }
            }
        }
    }
    bail!("an unsigned answer is a question");
}

fn verify_founder_signature(
    root: &Path,
    id: u64,
    kind: &str,
    body: &str,
    to: &str,
    outcome: &str,
    signature: &str,
    signer: &str,
    public_key: &str,
) -> Result<()> {
    if signature.is_empty() || signer.is_empty() || public_key.is_empty() {
        bail!("an unsigned answer is a question");
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(public_key)
        .unwrap_or_default();
    if raw.len() != 65 || raw.first() != Some(&4) {
        bail!("an unsigned answer is a question");
    }
    let fingerprint = fingerprint_of(&raw);
    let recorded = recorded_fingerprint(root).unwrap_or_default();
    if !signer.eq_ignore_ascii_case(&fingerprint) || !fingerprint.eq_ignore_ascii_case(&recorded) {
        bail!("an unsigned answer is a question");
    }
    let event = answer_event(id, kind, body, to, outcome);
    if event.contains("\"prompt\"") {
        bail!("an unsigned answer is a question");
    }
    let key = VerifyingKey::from_sec1_bytes(&raw)
        .map_err(|_| anyhow::anyhow!("an unsigned answer is a question"))?;
    let der = base64::engine::general_purpose::STANDARD
        .decode(signature)
        .map_err(|_| anyhow::anyhow!("an unsigned answer is a question"))?;
    let sig = Signature::from_der(&der)
        .map_err(|_| anyhow::anyhow!("an unsigned answer is a question"))?;
    key.verify(event.as_bytes(), &sig)
        .map_err(|_| anyhow::anyhow!("an unsigned answer is a question"))
}

fn answer_event(id: u64, kind: &str, body: &str, to: &str, outcome: &str) -> String {
    let mut event = format!(
        "{{\"id\":{id},\"agent\":\"{}\",\"kind\":{},\"body\":{}",
        OPERATOR,
        json_string(kind),
        json_string(body)
    );
    if !to.is_empty() {
        event.push_str(",\"to\":");
        event.push_str(&json_string(to));
    }
    if !outcome.is_empty() {
        event.push_str(",\"outcome\":");
        event.push_str(&json_string(outcome));
    }
    event.push('}');
    event
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '&' => out.push_str("\\u0026"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn fingerprint_of(raw: &[u8]) -> String {
    hex_encode(&Sha256::digest(raw))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn answer_desk(agent: &str, signature: Option<String>, signer: Option<String>) -> Result<DeskNeed> {
    let signature = signature.filter(|value| !value.is_empty());
    let signer = signer.filter(|value| !value.is_empty());
    if agent == "FOUNDER" && (signature.is_none() || signer.is_none()) {
        bail!("an unsigned answer is a question");
    }
    let desk = DeskNeed {
        signature,
        signer,
        ..DeskNeed::default()
    };
    if !desk.is_empty() {
        check_desk(&desk)?;
    }
    Ok(desk)
}

fn ack_op(
    record_id: u64,
    notice_id: u64,
    agent: String,
    outcome: &str,
    body: String,
    thread: Vec<u64>,
    desk: DeskNeed,
) -> Op {
    Op::Action {
        record_id,
        parent_id: Some(notice_id),
        kind: "ack".into(),
        body,
        mentions: vec![],
        status: None,
        blocked_by: None,
        clear_blocked_by: false,
        requested_agent: None,
        pr_id: None,
        merge_sha: None,
        merge_at: None,
        worktree: None,
        branch: None,
        clear_worktree: false,
        agent: Some(agent),
        pivot: None,
        to: None,
        outcome: Some(outcome.into()),
        thread,
        desk,
    }
}

/// Local calendar day of a ledger timestamp. NA uses one thread per day.
fn local_day(at: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|stamp| stamp.with_timezone(&Local).date_naive())
}

fn record_plan_ref(conn: &Connection, id: u64) -> Result<Option<String>> {
    match conn.query_row(
        "SELECT plan_ref FROM records WHERE id = ?1",
        params![id as i64],
        |row| row.get::<_, Option<String>>(0),
    ) {
        Ok(value) => Ok(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn na_day_head(conn: &Connection, record_id: u64, at: &str) -> Result<Option<u64>> {
    let Some(day) = local_day(at) else {
        return Ok(None);
    };
    let mut stmt = conn.prepare("SELECT id, at FROM actions WHERE record_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map(params![record_id as i64], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, when) = row?;
        if local_day(&when) == Some(day) {
            return Ok(Some(id as u64));
        }
    }
    Ok(None)
}

/// A new message on NA addresses that local day's first message.
fn stamp_na_day(conn: &Connection, op: &mut Op, at: &str) -> Result<()> {
    let Op::Action {
        record_id, thread, ..
    } = op
    else {
        return Ok(());
    };
    if record_plan_ref(conn, *record_id)?.as_deref() != Some("na") {
        return Ok(());
    }
    let Some(head) = na_day_head(conn, *record_id, at)? else {
        return Ok(());
    };
    if !thread.contains(&head) {
        thread.insert(0, head);
    }
    Ok(())
}

fn merge_thread(existing: &[u64], parent: Option<u64>, extra: &[u64]) -> Vec<u64> {
    let mut out = Vec::new();
    let mut push = |id: u64| {
        if id != 0 && !out.contains(&id) {
            out.push(id);
        }
    };
    if let Some(id) = parent {
        push(id);
    }
    for id in existing {
        push(*id);
    }
    for id in extra {
        push(*id);
    }
    out
}

fn shown_thread(stored: Vec<u64>, parent: Option<u64>) -> Vec<u64> {
    if stored.is_empty() {
        parent.into_iter().collect()
    } else {
        stored
    }
}

fn parse_ids(value: Option<String>) -> Vec<u64> {
    let Some(value) = value.filter(|text| !text.is_empty()) else {
        return Vec::new();
    };
    serde_json::from_str(&value).unwrap_or_default()
}

fn require_messages(conn: &Connection, ids: &[u64]) -> Result<()> {
    for id in ids {
        let found: bool = conn
            .query_row(
                "SELECT 1 FROM actions WHERE id = ?1",
                params![*id as i64],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if !found {
            bail!("thread #{id} is not a message");
        }
    }
    Ok(())
}

/// A channel post with no recipient is an older notice. It belongs on the board.
fn on_notice_board(recipient: Option<&str>) -> bool {
    match recipient {
        None | Some("") | Some(NOTICE_BOARD) => true,
        _ => false,
    }
}

fn list_messages(
    conn: &Connection,
    from: Option<&str>,
    to: Option<&str>,
    record: Option<&str>,
    all: bool,
    channel: bool,
) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.record_id, r.plan_ref, a.kind, a.agent, a.to_agent, a.at, a.body, a.parent_id, a.outcome, a.thread, a.desk
         FROM actions a
         JOIN records r ON r.id = a.record_id
         WHERE a.kind IN ('need', 'tell', 'handoff', 'ack', 'notice')
         ORDER BY a.id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)? as u64,
            row.get::<_, i64>(1)? as u64,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, Option<i64>>(8)?.map(|id| id as u64),
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
            row.get::<_, Option<String>>(11)?,
        ))
    })?;
    let mut raw = Vec::new();
    for row in rows {
        raw.push(row?);
    }
    let mut child_need = std::collections::BTreeSet::new();
    let mut sibling_tells = std::collections::BTreeSet::new();
    let mut acks: std::collections::BTreeMap<u64, (String, String)> =
        std::collections::BTreeMap::new();
    for (id, _, _, kind, _, _, _, body, parent, outcome, _, _) in &raw {
        if kind == "need" {
            if let Some(parent) = parent {
                child_need.insert(*parent);
            }
        }
        if kind == "tell" {
            if let Some(parent) = parent {
                sibling_tells.insert(*parent);
            }
        }
        let closes = kind == "ack"
            || (kind == "tell"
                && matches!(outcome.as_deref(), Some("moved" | "answered")));
        if closes {
            if let Some(parent) = parent {
                acks.insert(*parent, (outcome.clone().unwrap_or_default(), body.clone()));
            }
            let _ = id;
        }
    }
    let mut out = Vec::new();
    for (id, record_id, plan_ref, kind, agent, recipient, at, body, parent, outcome, stored, desk_json) in raw {
        if kind == "ack" {
            continue;
        }
        if kind == "handoff" && child_need.contains(&id) {
            continue;
        }
        if let Some(from) = from {
            if agent.as_deref() != Some(from) {
                continue;
            }
        }
        if let Some(to) = to {
            let matches = if to == NOTICE_BOARD && kind == "notice" {
                on_notice_board(recipient.as_deref())
            } else {
                recipient.as_deref() == Some(to)
            };
            if !matches {
                continue;
            }
        }
        if let Some(record) = record {
            let mut key = record.trim().trim_start_matches('#');
            if key.eq_ignore_ascii_case("na") {
                key = "na";
            }
            let id_match = key.parse::<u64>().ok() == Some(record_id);
            let ref_match = plan_ref.as_deref() == Some(key);
            if !id_match && !ref_match {
                continue;
            }
        }
        let (status, answer) = message_status(
            &kind,
            &at,
            acks.get(&id),
            sibling_tells.contains(&id),
        )?;
        let shown_to = if kind == "notice" && on_notice_board(recipient.as_deref()) {
            Some(NOTICE_BOARD.to_string())
        } else {
            recipient
        };
        if kind == "notice" {
            let board_query = to == Some(NOTICE_BOARD);
            if !all && !channel && !board_query {
                continue;
            }
        } else if !all && status != "open" && status != "overdue" {
            continue;
        }
        let answer_tell = kind == "tell"
            && is_answer_tell(
                &conn,
                &Action {
                    id,
                    record_id,
                    parent_id: parent,
                    at: at.clone(),
                    kind: kind.clone(),
                    body: body.clone(),
                    mentions: vec![],
                    agent: agent.clone(),
                    pivot: None,
                    to: shown_to.clone(),
                    outcome,
                    thread: vec![],
                },
            )?;
        out.push(Message {
            id,
            record_id,
            plan_ref,
            kind,
            from: agent,
            to: shown_to,
            at,
            status,
            body,
            answer,
            answer_tell,
            thread: shown_thread(parse_ids(stored), parent),
            desk: parse_desk(desk_json.as_deref()),
        });
    }
    Ok(out)
}

/// Keep every open notice, and only the newest channel posts.
fn trim_notices(rows: Vec<Message>, limit: u64) -> Vec<Message> {
    let mut notice_ids: Vec<u64> = rows
        .iter()
        .filter(|row| row.kind == "notice")
        .map(|row| row.id)
        .collect();
    if notice_ids.len() as u64 <= limit {
        return rows;
    }
    let drop_count = notice_ids.len() - limit as usize;
    notice_ids.truncate(drop_count);
    let drop: std::collections::BTreeSet<u64> = notice_ids.into_iter().collect();
    rows.into_iter()
        .filter(|row| row.kind != "notice" || !drop.contains(&row.id))
        .collect()
}

fn message_status(
    kind: &str,
    at: &str,
    ack: Option<&(String, String)>,
    legacy_reply: bool,
) -> Result<(String, Option<String>)> {
    if kind == "notice" {
        return Ok(("posted".into(), None));
    }
    let Some((outcome, body)) = ack else {
        if kind == "handoff" {
            return Ok(("sent".into(), None));
        }
        if kind == "need" {
            if let Ok(sent) = chrono::DateTime::parse_from_rfc3339(at) {
                if chrono::Utc::now().signed_duration_since(sent) > chrono::Duration::hours(24) {
                    return Ok(("overdue".into(), None));
                }
            }
        }
        return Ok(("open".into(), None));
    };
    let status = match outcome.as_str() {
        "did" if legacy_reply => "answered",
        "did" | "done" => "done",
        "moved" | "answered" => "answered",
        "later" => "later",
        "ask-founder" | "to_founder" => "to_founder",
        "ackn" => "ackn",
        "withdrawn" => "withdrawn",
        _ => bail!("unknown outcome {outcome}"),
    };
    Ok((status.into(), Some(body.clone())))
}

fn cancel_op(id: u64, agent: String, pivot: String, body: String, mentions: Vec<String>) -> Op {
    Op::Action {
        record_id: id,
        parent_id: None,
        kind: "supersede".into(),
        body,
        mentions,
        status: Some("cancelled".into()),
        blocked_by: None,
        clear_blocked_by: true,
        requested_agent: None,
        pr_id: None,
        merge_sha: None,
        merge_at: None,
        worktree: None,
        branch: None,
        clear_worktree: false,
        agent: Some(agent),
        pivot: Some(pivot),
        to: None,
        outcome: None,
        thread: vec![],
    
        desk: DeskNeed::default(),
}
}

fn write_event(conn: &Connection, log: &Path, mut op: Op) -> Result<Event> {
    let at = now_utc();
    stamp_na_day(conn, &mut op, &at)?;
    check_op(&op)?;
    let id = last_log_id(log)? + 1;
    let event = Event {
        v: SCHEMA_VERSION,
        id,
        at,
        op,
    };
    let previous_len = std::fs::metadata(log).map(|meta| meta.len()).unwrap_or(0);
    append_log(log, &event)?;
    let tx = conn.unchecked_transaction()?;
    let applied = (|| -> Result<()> {
        apply_event(&tx, &event).with_context(|| format!("apply event {id}"))?;
        set_meta(&tx, "last_id", &id.to_string())?;
        Ok(())
    })();
    match applied {
        Ok(()) => tx.commit()?,
        Err(err) => {
            let _ = tx.rollback();
            let file = OpenOptions::new().write(true).open(log)?;
            file.set_len(previous_len)?;
            file.sync_all()?;
            return Err(err);
        }
    }
    Ok(event)
}

fn block_settled(conn: &Connection, record_id: u64, plan_ref: &str) -> Result<bool> {
    let mut blocked_at = 0;
    for action in actions_for(conn, record_id)? {
        if action.kind == "block" {
            blocked_at = action.id;
        }
        if action.id > blocked_at
            && (action.kind == "note" || action.kind == "unblock")
            && names_plan(&action.body, &action.mentions, plan_ref)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn names_plan(body: &str, mentions: &[String], plan_ref: &str) -> bool {
    if plan_ref.is_empty() {
        return false;
    }
    if mentions.iter().any(|mention| mention == plan_ref) {
        return true;
    }
    body.match_indices(plan_ref).any(|(at, _)| {
        let boundary = |ch: char| !ch.is_ascii_alphanumeric();
        let before = body[..at].chars().next_back().is_none_or(boundary);
        let after = body[at + plan_ref.len()..]
            .chars()
            .next()
            .is_none_or(boundary);
        before && after
    })
}

fn settle_message(waiting: &[Record]) -> String {
    let plan_ref = waiting[0].blocked_by.as_deref().unwrap_or("this plan");
    let mut lines = vec![format!(
        "{plan_ref} still blocks {} records. Write a note or an unblock on each one that names {plan_ref}, then close again.",
        waiting.len()
    )];
    for record in waiting {
        let name = record
            .plan_ref
            .clone()
            .unwrap_or_else(|| record.kind.clone());
        lines.push(format!(
            "#{} {} {}  {}",
            record.id, name, record.status, record.summary
        ));
    }
    lines.join("\n")
}

fn actions_for(conn: &Connection, record_id: u64) -> Result<Vec<Action>> {
    let mut stmt = conn.prepare(
        "SELECT id, record_id, parent_id, at, kind, body, mentions, agent, pivot, to_agent, outcome, thread
         FROM actions WHERE record_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![record_id as i64], row_action)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn row_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
    Ok(Record {
        id: row.get::<_, i64>(0)? as u64,
        kind: row.get(1)?,
        plan_ref: row.get(2)?,
        plan_id: row.get::<_, Option<i64>>(3)?.map(|id| id as u64),
        repo: row.get(4)?,
        path: row.get(5)?,
        content_hash: row.get(6)?,
        worktree: row.get(7)?,
        branch: row.get(8)?,
        requesting_agent: row.get(9)?,
        requested_agent: row.get(10)?,
        summary: row.get(11)?,
        tags: parse_list(&row.get::<_, String>(12)?),
        entities: parse_list(&row.get::<_, String>(13)?),
        status: row.get(14)?,
        depends_on: parse_list(&row.get::<_, String>(15)?),
        blocked_by: row.get(16)?,
        pr_id: row.get(17)?,
        merge_sha: row.get(18)?,
        merge_at: row.get(19)?,
        opened_at: row.get(20)?,
    })
}

fn row_action(row: &rusqlite::Row<'_>) -> rusqlite::Result<Action> {
    let parent_id = row.get::<_, Option<i64>>(2)?.map(|id| id as u64);
    Ok(Action {
        id: row.get::<_, i64>(0)? as u64,
        record_id: row.get::<_, i64>(1)? as u64,
        parent_id,
        at: row.get(3)?,
        kind: row.get(4)?,
        body: row.get(5)?,
        mentions: parse_list(&row.get::<_, String>(6)?),
        agent: row.get(7)?,
        pivot: row.get(8)?,
        to: row.get(9)?,
        outcome: row.get(10)?,
        thread: shown_thread(parse_ids(row.get(11)?), parent_id),
    })
}

fn parse_list(value: &str) -> Vec<String> {
    serde_json::from_str(value).unwrap_or_default()
}

fn parse_desk(value: Option<&str>) -> DeskNeed {
    match value {
        Some(text) if !text.is_empty() => serde_json::from_str(text).unwrap_or_default(),
        _ => DeskNeed::default(),
    }
}

fn json<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    match conn.query_row(
        "SELECT value FROM meta WHERE key = ?1",
        params![key],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn read_log(path: &Path) -> Result<Vec<Event>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .with_context(|| format!("events.jsonl line {}", index + 1))?;
        let name = value.get("op").and_then(|item| item.as_str()).unwrap_or("");
        if !known_op(name) {
            continue;
        }
        let event: Event = serde_json::from_value(value)
            .with_context(|| format!("events.jsonl line {}", index + 1))?;
        events.push(event);
    }
    Ok(events)
}

fn last_log_id(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut max_id = 0u64;
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .with_context(|| format!("events.jsonl line {}", index + 1))?;
        if let Some(id) = value.get("id").and_then(|item| item.as_u64()) {
            max_id = max_id.max(id);
        }
    }
    Ok(max_id)
}

fn append_log(path: &Path, event: &Event) -> Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, event)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn owner_or_unknown(owners: &std::collections::BTreeMap<String, String>, repo: &str) -> String {
    owners
        .get(repo)
        .cloned()
        .unwrap_or_else(|| "unknown".into())
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf> {
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    full.canonicalize()
        .with_context(|| format!("plan file {}", full.display()))
}

pub fn fts_query(input: &str) -> Option<String> {
    let parts: Vec<String> = input
        .split_whitespace()
        .filter_map(|token| {
            let clean: String = token
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '/')
                .collect();
            if clean.is_empty() {
                None
            } else {
                Some(format!("\"{clean}\"*"))
            }
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" AND "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Op;
    use base64::Engine;
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::{Signature, SigningKey};

    fn fixture() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("owners.toml"),
            "example-platform = PLATFORM\nexample-app = APP\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"comms\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let store = Store::new(dir.path().to_path_buf());
        store.init().unwrap();
        (dir, store)
    }

    fn test_signing_key() -> SigningKey {
        SigningKey::from_slice(&[0x11; 32]).expect("test key")
    }

    fn test_public_raw() -> Vec<u8> {
        test_signing_key()
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec()
    }

    fn sign_founder(id: u64, kind: &str, body: &str, to: &str, outcome: &str) -> (String, String, String) {
        let event = answer_event(id, kind, body, to, outcome);
        let sig: Signature = test_signing_key().sign(event.as_bytes());
        let signature = base64::engine::general_purpose::STANDARD.encode(sig.to_der());
        (
            signature,
            fingerprint_of(&test_public_raw()),
            base64::engine::general_purpose::STANDARD.encode(test_public_raw()),
        )
    }

    fn signer_fixture() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ledger");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("owners.toml"),
            "example-platform = PLATFORM\nexample-app = APP\n",
        )
        .unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"comms\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let store = Store::new(root);
        store.init().unwrap();
        (dir, store)
    }

    fn record_signer(store_root: &Path) {
        let signs = store_root.join("signers");
        std::fs::create_dir_all(&signs).unwrap();
        let pin = fingerprint_of(&test_public_raw());
        std::fs::write(
            signs.join("founder.md"),
            format!("| Fingerprint | `{pin}` |\n"),
        )
        .unwrap();
    }

    fn plan_file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn dependency_order_and_replay() {
        let (dir, store) = fixture();
        let infra = plan_file(dir.path(), "infra.md", "example-database");
        let mobile = plan_file(dir.path(), "mobile.md", "chart");
        store
            .plan_open(
                "PLATFORM-15".into(),
                infra,
                "example-platform".into(),
                vec![],
                "Sample history".into(),
                vec!["schema".into()],
                vec!["example-database".into()],
            )
            .unwrap();
        store
            .plan_open(
                "APP-16".into(),
                mobile,
                "example-app".into(),
                vec!["PLATFORM-15".into()],
                "Sample chart".into(),
                vec!["chart".into()],
                vec![],
            )
            .unwrap();
        let ready = store.ready().unwrap();
        let refs: Vec<_> = ready
            .iter()
            .map(|plan| plan.record.plan_ref.clone().unwrap())
            .collect();
        assert_eq!(refs, vec!["PLATFORM-15".to_string()]);

        store
            .act(Op::Action {
                record_id: 1,
                parent_id: None,
                kind: "close".into(),
                body: "schema step 2 applied".into(),
                mentions: vec![],
                status: Some("closed".into()),
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: None,
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        let ready = store.ready().unwrap();
        assert_eq!(ready[0].record.plan_ref.as_deref(), Some("APP-16"));
        assert_eq!(ready[0].owner, "APP");

        let log = std::fs::read_to_string(dir.path().join("data/events.jsonl")).unwrap();
        assert!(!log.contains("author"));
        for name in ["comms.db", "comms.db-wal", "comms.db-shm"] {
            let path = dir.path().join("data").join(name);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        let rebuilt = Store::new(dir.path().to_path_buf());
        let show = rebuilt.show("APP-16").unwrap();
        assert_eq!(show.owner, "APP");
        assert_eq!(show.record.depends_on, vec!["PLATFORM-15".to_string()]);
        let hits = rebuilt.search("schema").unwrap();
        assert!(hits
            .iter()
            .any(|hit| hit.plan_ref.as_deref() == Some("PLATFORM-15")));
    }

    fn fixture_text(id: &str) -> String {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/notes-secret-refusal.json"
        ))
        .unwrap();
        let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
        doc["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == id)
            .unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn rebuild_replays_a_secret_shaped_notice_and_a_new_write_still_refuses() {
        let (dir, store) = fixture();
        let path = plan_file(dir.path(), "plan.md", "guards");
        store
            .plan_open(
                "IG-REPLAY".into(),
                path,
                "example-platform".into(),
                vec![],
                "Replay skips secret checks".into(),
                vec![],
                vec![],
            )
            .unwrap();
        store
            .notice(
                1,
                "RUN".into(),
                None,
                "Clean channel post.".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let secret = fixture_text("refuse-aws-key-id");
        let log_path = dir.path().join("data/events.jsonl");
        let raw = std::fs::read_to_string(&log_path).unwrap();
        let mut lines = Vec::new();
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let mut value: serde_json::Value = serde_json::from_str(line).unwrap();
            if value.get("kind").and_then(|kind| kind.as_str()) == Some("notice") {
                value["body"] = serde_json::Value::String(secret.clone());
            }
            lines.push(serde_json::to_string(&value).unwrap());
        }
        std::fs::write(&log_path, lines.join("\n") + "\n").unwrap();
        store.rebuild().unwrap();
        let messages = store.messages(None, None, Some("1"), true).unwrap();
        assert!(messages
            .iter()
            .any(|message| message.kind == "notice" && message.body == secret));
        let err = store
            .notice(1, "RUN".into(), None, secret.clone(), vec![], vec![])
            .unwrap_err()
            .to_string();
        assert!(err.contains("aws_key_id"), "{err}");
        assert!(err.contains("secret.put"), "{err}");
        assert!(!err.contains(&secret));
    }

    #[test]
    fn block_records_dependency_not_author() {
        let (dir, store) = fixture();
        let path = plan_file(dir.path(), "plan.md", "body");
        store
            .plan_open(
                "APP-16".into(),
                path,
                "example-app".into(),
                vec!["PLATFORM-15".into()],
                "Chart waits on history".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let event = store
            .item_open(Op::ItemOpen {
                plan_ref: "APP-16".into(),
                repo: "example-app".into(),
                worktree: Some("/tmp/wt-app-16".into()),
                branch: Some("sample/app-16".into()),
                requesting_agent: "APP".into(),
                requested_agent: "PLATFORM".into(),
                summary: "Implement the chart".into(),
                tags: vec!["chart".into()],
                entities: vec![],
            })
            .unwrap();
        store
            .act(Op::Action {
                record_id: event.id,
                parent_id: None,
                kind: "block".into(),
                body: "sample route still uses the prior name".into(),
                mentions: vec!["sample-read-history".into()],
                status: Some("blocked".into()),
                blocked_by: Some("PLATFORM-15".into()),
                clear_blocked_by: false,
                requested_agent: Some("PLATFORM".into()),
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: None,
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        let show = store.show("2").unwrap();
        assert_eq!(show.owner, "APP");
        assert_eq!(show.record.blocked_by.as_deref(), Some("PLATFORM-15"));
        assert_eq!(show.actions.len(), 1);
        let hits = store.search("sample-read-history").unwrap();
        assert_eq!(hits[0].record_id, 2);
    }

    fn close_plan(store: &Store, id: u64) -> anyhow::Result<crate::model::Event> {
        store.act(Op::Action {
            record_id: id,
            parent_id: None,
            kind: "close".into(),
            body: "closed".into(),
            mentions: vec![],
            status: Some("closed".into()),
            blocked_by: None,
            clear_blocked_by: false,
            requested_agent: None,
            pr_id: None,
            merge_sha: None,
            merge_at: None,
            worktree: None,
            branch: None,
            clear_worktree: false,
            agent: None,
            pivot: None,
            to: None,
            outcome: None,
            thread: vec![],
        
            desk: DeskNeed::default(),
})
    }

    #[test]
    fn close_waits_until_blocked_records_are_named() {
        let (dir, store) = fixture();
        let infra = plan_file(dir.path(), "infra.md", "vpc");
        let mobile = plan_file(dir.path(), "mobile.md", "card");
        store
            .plan_open(
                "PLATFORM-24".into(),
                infra,
                "example-platform".into(),
                vec![],
                "Management VPC".into(),
                vec![],
                vec![],
            )
            .unwrap();
        store
            .plan_open(
                "APP-36".into(),
                mobile,
                "example-app".into(),
                vec!["PLATFORM-24".into()],
                "Parse the category".into(),
                vec![],
                vec![],
            )
            .unwrap();
        store
            .act(Op::Action {
                record_id: 2,
                parent_id: None,
                kind: "block".into(),
                body: "waits on the field".into(),
                mentions: vec![],
                status: Some("blocked".into()),
                blocked_by: Some("PLATFORM-24".into()),
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: None,
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        let err = close_plan(&store, 1).unwrap_err().to_string();
        assert!(err.contains("#2 APP-36"), "{err}");
        store
            .act(Op::Action {
                record_id: 2,
                parent_id: None,
                kind: "note".into(),
                body: "still working".into(),
                mentions: vec![],
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: None,
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        assert!(close_plan(&store, 1).is_err());
        store
            .act(Op::Action {
                record_id: 2,
                parent_id: None,
                kind: "note".into(),
                body: "PLATFORM-24 is closed. APP confirms the field, then unblocks.".into(),
                mentions: vec![],
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: None,
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        close_plan(&store, 1).unwrap();
        assert_eq!(store.show("PLATFORM-24").unwrap().record.status, "closed");
        assert_eq!(store.show("2").unwrap().record.status, "blocked");
    }

    #[test]
    fn supersede_cancels_the_plan_and_its_open_item() {
        let (dir, store) = fixture();
        let infra = plan_file(dir.path(), "infra.md", "vpc");
        let mobile = plan_file(dir.path(), "mobile.md", "card");
        let next = plan_file(dir.path(), "next.md", "pivot");
        let waiting = plan_file(dir.path(), "wait.md", "depends");
        store
            .plan_open(
                "PLATFORM-24".into(),
                infra,
                "example-platform".into(),
                vec![],
                "Management VPC".into(),
                vec![],
                vec![],
            )
            .unwrap();
        store
            .item_open(Op::ItemOpen {
                plan_ref: "PLATFORM-24".into(),
                repo: "example-platform".into(),
                worktree: None,
                branch: None,
                requesting_agent: "LEAD".into(),
                requested_agent: "PLATFORM".into(),
                summary: "Host the dashboard".into(),
                tags: vec![],
                entities: vec![],
            })
            .unwrap();
        store
            .plan_open(
                "APP-36".into(),
                mobile,
                "example-app".into(),
                vec![],
                "Parse the category".into(),
                vec![],
                vec![],
            )
            .unwrap();
        store
            .plan_open(
                "APP-40".into(),
                next,
                "example-app".into(),
                vec![],
                "Replacement board".into(),
                vec![],
                vec![],
            )
            .unwrap();
        store
            .plan_open(
                "APP-41".into(),
                waiting,
                "example-app".into(),
                vec!["PLATFORM-24".into()],
                "Still depends on the old plan".into(),
                vec![],
                vec![],
            )
            .unwrap();
        store
            .act(Op::Action {
                record_id: 3,
                parent_id: None,
                kind: "block".into(),
                body: "waits on the field".into(),
                mentions: vec![],
                status: Some("blocked".into()),
                blocked_by: Some("PLATFORM-24".into()),
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: None,
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        let err = store
            .supersede(
                1,
                "PLATFORM".into(),
                "APP-40".into(),
                "VPC moved under the replacement.".into(),
                vec![],
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("#3 APP-36"), "{err}");
        store
            .act(Op::Action {
                record_id: 3,
                parent_id: None,
                kind: "note".into(),
                body: "PLATFORM-24 is cancelled. APP re-points this.".into(),
                mentions: vec![],
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: None,
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        store
            .supersede(
                1,
                "PLATFORM".into(),
                "APP-40".into(),
                "VPC moved under the replacement.".into(),
                vec![],
            )
            .unwrap();
        let plan = store.show("PLATFORM-24").unwrap();
        let item = store.show("2").unwrap();
        assert_eq!(plan.record.status, "cancelled");
        assert_eq!(item.record.status, "cancelled");
        assert_eq!(store.show("APP-36").unwrap().record.status, "blocked");
        for show in [plan, item] {
            let action = show.actions.last().unwrap();
            assert_eq!(action.kind, "supersede");
            assert_eq!(action.agent.as_deref(), Some("PLATFORM"));
            assert_eq!(action.pivot.as_deref(), Some("APP-40"));
            assert_eq!(action.body, "VPC moved under the replacement.");
        }
        let ready: Vec<_> = store
            .ready()
            .unwrap()
            .iter()
            .filter_map(|plan| plan.record.plan_ref.clone())
            .collect();
        assert!(
            !ready.iter().any(|plan_ref| plan_ref == "APP-41"),
            "{ready:?}"
        );
        assert!(store
            .supersede(1, "PLATFORM".into(), "APP-40".into(), "again".into(), vec![])
            .is_err());
    }

    #[test]
    fn handoff_sends_a_need_and_messages_filter_by_party() {
        let (dir, store) = fixture();
        let path = plan_file(dir.path(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-24".into(),
                path,
                "example-platform".into(),
                vec![],
                "Management VPC".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let events = store
            .handoff(
                1,
                "LEAD".into(),
                "PLATFORM".into(),
                None,
                "Apply the VPC plan.".into(),
                vec![],
            )
            .unwrap();
        assert_eq!(events.len(), 2);
        assert!(store.show("1").unwrap().record.requested_agent.as_deref() == Some("PLATFORM"));
        let all = store.messages(None, None, None, false).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].kind, "need");
        assert_eq!(all[0].from.as_deref(), Some("LEAD"));
        assert_eq!(all[0].to.as_deref(), Some("PLATFORM"));
        assert_eq!(all[0].status, "open");
        assert!(
            store
                .messages(Some("LEAD"), None, None, false)
                .unwrap()
                .len()
                == 1
        );
        assert!(store
            .messages(None, Some("APP"), None, false)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .messages(None, None, Some("PLATFORM-24"), false)
                .unwrap()
                .len(),
            1
        );
        assert!(store.messages(None, None, Some("#1"), false).unwrap().len() == 1);
        assert!(store
            .messages(None, None, Some("other"), false)
            .unwrap()
            .is_empty());
        let again = store
            .handoff(
                1,
                "PLATFORM".into(),
                "GUARD".into(),
                None,
                "Review the plan.".into(),
                vec![],
            )
            .unwrap();
        match &again[0].op {
            Op::Action { kind, .. } => assert_eq!(kind, "ack"),
            other => panic!("expected an ack, got {other:?}"),
        }
        assert_eq!(
            store.messages(None, Some("PLATFORM"), None, true).unwrap()[0].status,
            "done"
        );
        let sea = store.messages(None, Some("GUARD"), None, false).unwrap();
        assert_eq!(sea.len(), 1);
        assert_eq!(sea[0].body, "Review the plan.");
        assert!(store
            .ack(sea[0].id, "LEAD".into(), "did".into(), "no".into(), vec![])
            .is_err());
        assert_eq!(
            store
                .ack(sea[0].id, "GUARD".into(), "did".into(), "Reviewed.".into(), vec![])
                .unwrap()
                .len(),
            1
        );
        store
            .act(Op::Action {
                record_id: 1,
                parent_id: None,
                kind: "handoff".into(),
                body: "older handoff".into(),
                mentions: vec![],
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: Some("FOUNDER".into()),
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: None,
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        assert!(store
            .messages(None, Some("FOUNDER"), Some("PLATFORM-24"), false)
            .unwrap()
            .is_empty());
        let older = store
            .messages(None, Some("FOUNDER"), Some("PLATFORM-24"), true)
            .unwrap();
        assert_eq!(older.len(), 1);
        assert_eq!(older[0].to.as_deref(), Some("FOUNDER"));
        assert!(older[0].from.is_none());
    }

    #[test]
    fn tell_all_reaches_agents_with_open_work() {
        let (dir, store) = fixture();
        let path = plan_file(dir.path(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-24".into(),
                path,
                "example-platform".into(),
                vec![],
                "Management VPC".into(),
                vec![],
                vec![],
            )
            .unwrap();
        assert!(store
            .tell(
                1,
                "LEAD".into(),
                None,
                true,
                "tell".into(),
                "Nobody is assigned.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .is_err());
        store
            .item_open(Op::ItemOpen {
                plan_ref: "PLATFORM-24".into(),
                repo: "example-platform".into(),
                worktree: None,
                branch: None,
                requesting_agent: "LEAD".into(),
                requested_agent: "PLATFORM".into(),
                summary: "Open for PLATFORM".into(),
                tags: vec![],
                entities: vec![],
            })
            .unwrap();
        store
            .item_open(Op::ItemOpen {
                plan_ref: "PLATFORM-24".into(),
                repo: "example-app".into(),
                worktree: None,
                branch: None,
                requesting_agent: "LEAD".into(),
                requested_agent: "APP".into(),
                summary: "Open for APP".into(),
                tags: vec![],
                entities: vec![],
            })
            .unwrap();
        store
            .item_open(Op::ItemOpen {
                plan_ref: "PLATFORM-24".into(),
                repo: "example-platform".into(),
                worktree: None,
                branch: None,
                requesting_agent: "LEAD".into(),
                requested_agent: "FOUNDER".into(),
                summary: "The operator is not on --all".into(),
                tags: vec![],
                entities: vec![],
            })
            .unwrap();
        let events = store
            .tell(
                1,
                "LEAD".into(),
                None,
                true,
                "tell".into(),
                "Stay off Lambda.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        assert_eq!(events.len(), 2);
        let tos: Vec<_> = store
            .messages(Some("LEAD"), None, None, false)
            .unwrap()
            .into_iter()
            .map(|row| row.to.unwrap())
            .collect();
        assert_eq!(tos, vec!["APP", "PLATFORM"]);
        assert!(store
            .tell(
                1,
                "LEAD".into(),
                Some("PLATFORM".into()),
                true,
                "tell".into(),
                "both".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .is_err());
        assert!(store
            .tell(
                1,
                "LEAD".into(),
                Some("LEAD".into()),
                false,
                "tell".into(),
                "self".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .is_err());
        store
            .tell(
                1,
                "LEAD".into(),
                Some("APP".into()),
                false,
                "need".into(),
                "Confirm the sheet.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        let mob = store.messages(None, Some("APP"), None, false).unwrap();
        assert_eq!(mob.len(), 2);
        assert!(mob
            .iter()
            .any(|row| row.kind == "need" && row.body == "Confirm the sheet."));
    }

    #[test]
    fn unblock_and_a_closed_dependency_wake_the_assignee() {
        let (dir, store) = fixture();
        let infra = plan_file(dir.path(), "infra.md", "body");
        let mobile = plan_file(dir.path(), "mobile.md", "body");
        store
            .plan_open(
                "PLATFORM-15".into(),
                infra,
                "example-platform".into(),
                vec![],
                "History".into(),
                vec![],
                vec![],
            )
            .unwrap();
        store
            .plan_open(
                "APP-16".into(),
                mobile,
                "example-app".into(),
                vec!["PLATFORM-15".into()],
                "Chart".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let item = store
            .item_open(Op::ItemOpen {
                plan_ref: "PLATFORM-15".into(),
                repo: "example-platform".into(),
                worktree: None,
                branch: None,
                requesting_agent: "LEAD".into(),
                requested_agent: "PLATFORM".into(),
                summary: "Apply the migration".into(),
                tags: vec![],
                entities: vec![],
            })
            .unwrap();
        store
            .act(Op::Action {
                record_id: item.id,
                parent_id: None,
                kind: "block".into(),
                body: "waiting".into(),
                mentions: vec![],
                status: Some("blocked".into()),
                blocked_by: Some("APP-16".into()),
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some("LEAD".into()),
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        store
            .act(Op::Action {
                record_id: item.id,
                parent_id: None,
                kind: "unblock".into(),
                body: "unblocked".into(),
                mentions: vec![],
                status: Some("open".into()),
                blocked_by: None,
                clear_blocked_by: true,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some("LEAD".into()),
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        let inf = store.messages(None, Some("PLATFORM"), None, false).unwrap();
        assert_eq!(inf.len(), 1);
        assert_eq!(inf[0].kind, "need");
        assert!(inf[0].body.contains("unblocked"));
        assert!(store
            .ack(inf[0].id, "PLATFORM".into(), "ackn".into(), "read".into(), vec![])
            .is_err());
        store
            .act(Op::Action {
                record_id: 1,
                parent_id: None,
                kind: "close".into(),
                body: "closed".into(),
                mentions: vec![],
                status: Some("closed".into()),
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some("LEAD".into()),
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        let mob = store.messages(None, Some("APP"), None, false).unwrap();
        assert!(mob
            .iter()
            .any(|row| row.kind == "need" && row.body.contains("ready")));
        store
            .act(Op::Action {
                record_id: item.id,
                parent_id: None,
                kind: "unblock".into(),
                body: "again".into(),
                mentions: vec![],
                status: Some("open".into()),
                blocked_by: None,
                clear_blocked_by: true,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some("PLATFORM".into()),
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
            
                desk: DeskNeed::default(),
})
            .unwrap();
        assert_eq!(
            store.messages(None, Some("PLATFORM"), None, false).unwrap().len(),
            1
        );
    }

    #[test]
    fn reply_answers_and_tells_the_sender() {
        let (dir, store) = fixture();
        let path = plan_file(dir.path(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-24".into(),
                path,
                "example-platform".into(),
                vec![],
                "Management VPC".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let sent = store
            .tell(
                1,
                "LEAD".into(),
                Some("FOUNDER".into()),
                false,
                "tell".into(),
                "Fleet is paused.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        let events = store
            .reply(
                sent[0].id,
                "FOUNDER".into(),
                "Understood. I will leave it paused.".into(),
                vec![],
            )
            .unwrap();
        assert_eq!(events.len(), 2);
        let back = store.messages(None, Some("LEAD"), None, false).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].from.as_deref(), Some("FOUNDER"));
        assert_eq!(back[0].body, "Understood. I will leave it paused.");
        assert!(back[0].answer_tell);
        assert_eq!(back[0].thread, vec![sent[0].id]);
        let refused = store
            .ack(
                back[0].id,
                "LEAD".into(),
                "ackn".into(),
                "read".into(),
                vec![],
            )
            .unwrap_err()
            .to_string();
        assert!(refused.contains("not a fact"), "{refused}");
        let withdrawn = store
            .withdraw(back[0].id, "FOUNDER".into(), "undo".into())
            .unwrap_err()
            .to_string();
        assert!(withdrawn.contains("answer tell"), "{withdrawn}");
        let conn = rusqlite::Connection::open(store.db_path()).unwrap();
        conn.execute(
            "UPDATE actions SET thread = NULL WHERE id = ?1",
            [back[0].id as i64],
        )
        .unwrap();
        drop(conn);
        let legacy = store.messages(None, Some("LEAD"), None, false).unwrap();
        assert_eq!(legacy[0].thread, vec![sent[0].id]);
        store
            .link_thread(back[0].id, "LEAD".into(), vec![sent[0].id])
            .unwrap();
        let linked = store.messages(None, Some("LEAD"), None, false).unwrap();
        assert_eq!(linked[0].thread, vec![sent[0].id]);
        let mine = store.messages(None, Some("FOUNDER"), None, true).unwrap();
        assert_eq!(mine[0].status, "answered");
        assert_eq!(
            mine[0].answer.as_deref(),
            Some("Understood. I will leave it paused.")
        );
        assert!(store
            .reply(sent[0].id, "FOUNDER".into(), "again".into(), vec![])
            .is_err());
        let other = store
            .tell(
                1,
                "LEAD".into(),
                Some("PLATFORM".into()),
                false,
                "tell".into(),
                "For PLATFORM.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        assert!(store
            .reply(other[0].id, "FOUNDER".into(), "no".into(), vec![])
            .is_err());
    }

    #[test]
    fn ackn_closes_a_tell_and_refuses_a_need() {
        let (dir, store) = fixture();
        let path = plan_file(dir.path(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-24".into(),
                path,
                "example-platform".into(),
                vec![],
                "Management VPC".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let sent = store
            .tell(
                1,
                "LEAD".into(),
                Some("FOUNDER".into()),
                false,
                "tell".into(),
                "Fleet is paused.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        store
            .ack(
                sent[0].id,
                "FOUNDER".into(),
                "ackn".into(),
                "acknowledged".into(),
                vec![],
            )
            .unwrap();
        assert!(store
            .messages(None, Some("FOUNDER"), None, false)
            .unwrap()
            .is_empty());
        let closed = store.messages(None, Some("FOUNDER"), None, true).unwrap();
        assert_eq!(closed[0].status, "ackn");
        let need = store
            .tell(
                1,
                "LEAD".into(),
                Some("FOUNDER".into()),
                false,
                "need".into(),
                "Confirm the pause.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        let err = store
            .ack(need[0].id, "FOUNDER".into(), "ackn".into(), "no".into(), vec![])
            .unwrap_err()
            .to_string();
        assert!(err.contains("need"), "{err}");
    }

    #[test]
    fn a_legacy_reply_ack_projects_as_answered() {
        let (dir, store) = fixture();
        let path = plan_file(dir.path(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-24".into(),
                path,
                "example-platform".into(),
                vec![],
                "Management VPC".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let sent = store
            .tell(
                1,
                "LEAD".into(),
                Some("FOUNDER".into()),
                false,
                "need".into(),
                "Ship the file.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        store
            .act(Op::Action {
                record_id: 1,
                parent_id: Some(sent[0].id),
                kind: "ack".into(),
                body: "Ship the file.".into(),
                mentions: vec![],
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some("FOUNDER".into()),
                pivot: None,
                to: None,
                outcome: Some("did".into()),
                thread: vec![sent[0].id],
                desk: DeskNeed::default(),
            })
            .unwrap();
        store
            .act(Op::Action {
                record_id: 1,
                parent_id: Some(sent[0].id),
                kind: "tell".into(),
                body: "Ship the file.".into(),
                mentions: vec![],
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: Some("FOUNDER".into()),
                pivot: None,
                to: Some("LEAD".into()),
                outcome: None,
                thread: vec![sent[0].id],
                desk: DeskNeed::default(),
            })
            .unwrap();
        let row = store
            .messages(None, Some("FOUNDER"), None, true)
            .unwrap()
            .into_iter()
            .find(|row| row.id == sent[0].id)
            .unwrap();
        assert_eq!(row.status, "answered");
        let back = store.messages(None, Some("LEAD"), None, false).unwrap();
        let err = store
            .ack(back[0].id, "LEAD".into(), "ackn".into(), "read".into(), vec![])
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a fact"), "{err}");
    }

    #[test]
    fn the_sender_withdraws_an_open_need() {
        let (dir, store) = fixture();
        let path = plan_file(dir.path(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-24".into(),
                path,
                "example-platform".into(),
                vec![],
                "Management VPC".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let sent = store
            .tell(
                1,
                "LEAD".into(),
                Some("FOUNDER".into()),
                false,
                "need".into(),
                "Old desk order.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        let err = store
            .withdraw(sent[0].id, "FOUNDER".into(), "not mine".into())
            .unwrap_err()
            .to_string();
        assert!(err.contains("from LEAD"), "{err}");
        let err = store
            .ack(sent[0].id, "FOUNDER".into(), "nope".into(), "x".into(), vec![])
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown outcome"), "{err}");
        store
            .withdraw(sent[0].id, "LEAD".into(), "superseded".into())
            .unwrap();
        let row = store
            .messages(None, Some("FOUNDER"), None, true)
            .unwrap()
            .into_iter()
            .find(|row| row.id == sent[0].id)
            .unwrap();
        assert_eq!(row.status, "withdrawn");
        assert!(store
            .messages(None, Some("FOUNDER"), None, false)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn na_is_one_cancelled_record() {
        let (dir, store) = fixture();
        std::fs::write(
            dir.path().join("owners.toml"),
            "example-platform = PLATFORM\nledger = LEAD\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("README.md"), "comms\n").unwrap();
        let first = store.resolve_on("NA").unwrap();
        let second = store.resolve_on("na").unwrap();
        assert_eq!(first, second);
        let show = store.show("na").unwrap();
        assert_eq!(show.record.status, "cancelled");
        assert!(store.ready().unwrap().is_empty());
        store
            .tell(
                first,
                "PLATFORM".into(),
                Some("LEAD".into()),
                false,
                "tell".into(),
                "No project.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        let mine = store.messages(None, Some("LEAD"), None, false).unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].record_id, first);
    }

    #[test]
    fn na_messages_share_one_thread_per_day() {
        let (dir, store) = fixture();
        std::fs::write(
            dir.path().join("owners.toml"),
            "example-platform = PLATFORM\nledger = LEAD\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("README.md"), "comms\n").unwrap();
        let record = store.resolve_on("NA").unwrap();
        let first = store
            .tell(
                record,
                "PLATFORM".into(),
                Some("LEAD".into()),
                false,
                "tell".into(),
                "Morning.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        let second = store
            .tell(
                record,
                "PLATFORM".into(),
                Some("LEAD".into()),
                false,
                "tell".into(),
                "Afternoon.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        let head = first[0].id;
        match &second[0].op {
            Op::Action { thread, .. } => assert_eq!(thread, &vec![head]),
            other => panic!("expected an action, got {other:?}"),
        }
        let rows = store.messages(None, None, Some("na"), true).unwrap();
        let later = rows.iter().find(|row| row.id == second[0].id).unwrap();
        assert!(later.thread.contains(&head), "{:?}", later.thread);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn a_channel_post_is_not_inbox_mail() {
        let (dir, store) = fixture();
        let path = plan_file(dir.path(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-24".into(),
                path,
                "example-platform".into(),
                vec![],
                "A plan".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let posted = store
            .notice(1, "LEAD".into(), None, "Status, no action.".into(), vec![], vec![])
            .unwrap();
        assert!(store.messages(None, None, None, false).unwrap().is_empty());
        assert!(store
            .messages(None, Some("FOUNDER"), None, false)
            .unwrap()
            .is_empty());
        let feed = store.channel_messages(None, None, None, false, 3).unwrap();
        assert_eq!(feed.len(), 1);
        assert_eq!(feed[0].id, posted[0].id);
        assert_eq!(feed[0].kind, "notice");
        assert_eq!(feed[0].status, "posted");
        assert_eq!(feed[0].to.as_deref(), Some("NB"));
        let board = store.messages(None, Some("NB"), None, false).unwrap();
        assert!(board.iter().any(|row| row.id == posted[0].id));
        assert!(store
            .messages(None, Some("LEAD"), None, false)
            .unwrap()
            .is_empty());
        let err = store
            .notice(
                1,
                "LEAD".into(),
                Some("APP".into()),
                "Not a board post.".into(),
                vec![],
                vec![],
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("NB"), "{err}");
        let conn = rusqlite::Connection::open(store.db_path()).unwrap();
        conn.execute(
            "UPDATE actions SET to_agent = NULL WHERE id = ?1",
            [posted[0].id as i64],
        )
        .unwrap();
        drop(conn);
        let legacy = store.messages(None, Some("NB"), None, false).unwrap();
        let old = legacy.iter().find(|row| row.id == posted[0].id).unwrap();
        assert_eq!(old.to.as_deref(), Some("NB"));
        for n in 0..3 {
            store
                .notice(1, "LEAD".into(), None, format!("Post {n}."), vec![], vec![])
                .unwrap();
        }
        let newest = store.channel_messages(None, None, None, false, 3).unwrap();
        let posted_ids: Vec<u64> = newest
            .iter()
            .filter(|row| row.kind == "notice")
            .map(|row| row.id)
            .collect();
        assert_eq!(posted_ids.len(), 3);
        assert!(!posted_ids.contains(&posted[0].id));
        let one = store.channel_messages(None, None, None, false, 1).unwrap();
        assert_eq!(
            one.iter().filter(|row| row.kind == "notice").count(),
            1
        );
        let err = store
            .ack(
                posted[0].id,
                "FOUNDER".into(),
                "ackn".into(),
                "read".into(),
                vec![],
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("channel post"), "{err}");
        let before = crate::listen::ListenSnap::capture(&store, "LEAD").unwrap();
        store
            .notice(1, "LEAD".into(), None, "Second post.".into(), vec![], vec![])
            .unwrap();
        let after = crate::listen::ListenSnap::capture(&store, "LEAD").unwrap();
        assert!(!crate::listen::inbox_wakes("LEAD", &before, &after));
        assert!(crate::listen::inbox_wakes("FOUNDER", &before, &after));
    }

    #[test]
    fn an_unknown_op_is_skipped_and_a_desk_answer_is_one_event() {
        let (_dir, store) = signer_fixture();
        let path = plan_file(store.root(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-30".into(),
                path,
                "example-platform".into(),
                vec![],
                "Desk fields".into(),
                vec![],
                vec![],
            )
            .unwrap();
        record_signer(store.root());
        let sent = store
            .tell(
                1,
                "PLATFORM".into(),
                Some("FOUNDER".into()),
                false,
                "need".into(),
                "Choose a size.".into(),
                vec![],
                vec![],
                DeskNeed {
                    urgency: Some(2),
                    category: Some("cost".into()),
                    title: Some("Database sizing".into()),
                    why: Some("The paid tier is closed.".into()),
                    ..DeskNeed::default()
                },
            )
            .unwrap();
        let open = store.messages(None, Some("FOUNDER"), None, false).unwrap();
        assert_eq!(open[0].desk.urgency, Some(2));
        assert_eq!(open[0].desk.title.as_deref(), Some("Database sizing"));
        let (signature, signer, public_key) = sign_founder(sent[0].id, "ack", "small tier", "", "did");
        let event = store
            .answer(
                sent[0].id,
                "FOUNDER".into(),
                "ack".into(),
                "small tier".into(),
                None,
                Some("did".into()),
                vec![],
                Some(signature),
                Some(signer),
                Some(public_key),
            )
            .unwrap();
        match &event.op {
            Op::Action { kind, .. } => assert_eq!(kind, "ack"),
            other => panic!("expected an ack, got {other:?}"),
        }
        let back = store.messages(None, Some("PLATFORM"), None, false).unwrap();
        assert!(back.is_empty());
        let line = format!(
            "{{\"v\":1,\"id\":{},\"at\":\"2026-10-11T00:00:00Z\",\"op\":\"future_op\"}}\n",
            event.id + 1
        );
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(store.log_path())
            .unwrap();
        std::io::Write::write_all(&mut file, line.as_bytes()).unwrap();
        let reopened = Store::new(store.root().to_path_buf());
        reopened.messages(None, Some("FOUNDER"), None, true).unwrap();
        let next = reopened
            .act(Op::Action {
                record_id: 1,
                parent_id: None,
                kind: "note".into(),
                body: "still writes".into(),
                mentions: vec![],
                status: None,
                blocked_by: None,
                clear_blocked_by: false,
                requested_agent: None,
                pr_id: None,
                merge_sha: None,
                merge_at: None,
                worktree: None,
                branch: None,
                clear_worktree: false,
                agent: None,
                pivot: None,
                to: None,
                outcome: None,
                thread: vec![],
                desk: DeskNeed::default(),
            })
            .unwrap();
        assert_eq!(next.id, event.id + 2);
    }

    #[test]
    fn a_moved_reply_closes_the_need_without_marking_it_done() {
        let (_dir, store) = signer_fixture();
        let path = plan_file(store.root(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-31".into(),
                path,
                "example-platform".into(),
                vec![],
                "Queue note".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let sent = store
            .tell(
                1,
                "PLATFORM".into(),
                Some("FOUNDER".into()),
                false,
                "need".into(),
                "Place the file at the given path.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        record_signer(store.root());
        let (signature, signer, public_key) =
            sign_founder(sent[0].id, "tell", "moved to note 44", "PLATFORM", "moved");
        let event = store
            .answer(
                sent[0].id,
                "FOUNDER".into(),
                "tell".into(),
                "moved to note 44".into(),
                Some("PLATFORM".into()),
                Some("moved".into()),
                vec![],
                Some(signature),
                Some(signer),
                Some(public_key),
            )
            .unwrap();
        match &event.op {
            Op::Action { kind, outcome, body, .. } => {
                assert_eq!(kind, "tell");
                assert_eq!(outcome.as_deref(), Some("moved"));
                assert!(!body.contains("given path"));
            }
            other => panic!("expected a reply, got {other:?}"),
        }
        let open = store.messages(None, Some("FOUNDER"), None, false).unwrap();
        assert!(open.is_empty());
        let all = store.messages(None, Some("FOUNDER"), None, true).unwrap();
        let row = all.iter().find(|row| row.id == sent[0].id).unwrap();
        assert_eq!(row.status, "answered");
        let signed = store
            .withdraw(event.id, "FOUNDER".into(), "undo".into())
            .unwrap_err()
            .to_string();
        assert!(signed.contains("answer tell"), "{signed}");
        let err = store
            .answer(
                sent[0].id,
                "FOUNDER".into(),
                "ack".into(),
                "complete".into(),
                None,
                Some("moved".into()),
                vec![],
                Some("c2ln".into()),
                Some("abcd".into()),
                None,
            )
            .unwrap_err();
        assert!(err.to_string().contains("moved is a reply"), "{err}");
    }

    #[test]
    fn done_closes_an_answer_tell() {
        let (_dir, store) = signer_fixture();
        let path = plan_file(store.root(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-33".into(),
                path,
                "example-platform".into(),
                vec![],
                "Done close".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let sent = store
            .tell(
                1,
                "FOUNDER".into(),
                Some("LEAD".into()),
                false,
                "need".into(),
                "Where is the sprint?".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        let replied = store
            .reply(sent[0].id, "LEAD".into(), "No further action.".into(), vec![])
            .unwrap();
        let answer_id = replied[1].id;
        let open = store.messages(None, Some("FOUNDER"), None, false).unwrap();
        assert!(open.iter().any(|row| row.id == answer_id && row.answer_tell));
        record_signer(store.root());
        let (signature, signer, public_key) = sign_founder(answer_id, "ack", "done", "", "did");
        let event = store
            .answer(
                answer_id,
                "FOUNDER".into(),
                "ack".into(),
                "done".into(),
                None,
                Some("did".into()),
                vec![],
                Some(signature),
                Some(signer),
                Some(public_key),
            )
            .unwrap();
        match &event.op {
            Op::Action { kind, outcome, .. } => {
                assert_eq!(kind, "ack");
                assert_eq!(outcome.as_deref(), Some("did"));
            }
            other => panic!("expected an ack, got {other:?}"),
        }
        let still = store.messages(None, Some("FOUNDER"), None, false).unwrap();
        assert!(!still.iter().any(|row| row.id == answer_id));
        let fact = store
            .tell(
                1,
                "LEAD".into(),
                Some("FOUNDER".into()),
                false,
                "tell".into(),
                "A fact.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        let (signature, signer, public_key) = sign_founder(fact[0].id, "ack", "done", "", "did");
        let err = store
            .answer(
                fact[0].id,
                "FOUNDER".into(),
                "ack".into(),
                "done".into(),
                None,
                Some("did".into()),
                vec![],
                Some(signature),
                Some(signer),
                Some(public_key),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("is a fact"), "{err}");
    }

    #[test]
    fn a_signed_reply_on_a_tell_writes_the_need_before_the_ack() {
        let (_dir, store) = signer_fixture();
        let path = plan_file(store.root(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-32".into(),
                path,
                "example-platform".into(),
                vec![],
                "Tell reply".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let sent = store
            .tell(
                1,
                "PLATFORM".into(),
                Some("FOUNDER".into()),
                false,
                "tell".into(),
                "Counts are in.".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        record_signer(store.root());
        let unsigned = store
            .answer(
                sent[0].id,
                "FOUNDER".into(),
                "need".into(),
                "Ship it.".into(),
                Some("PLATFORM".into()),
                Some("answered".into()),
                vec![],
                None,
                None,
                None,
            )
            .unwrap_err()
            .to_string();
        assert!(unsigned.contains("unsigned"), "{unsigned}");
        assert_eq!(
            store
                .messages(None, Some("FOUNDER"), None, false)
                .unwrap()
                .len(),
            1
        );
        let (signature, signer, public_key) =
            sign_founder(sent[0].id, "need", "Ship it.", "PLATFORM", "answered");
        let need = store
            .answer(
                sent[0].id,
                "FOUNDER".into(),
                "need".into(),
                "Ship it.".into(),
                Some("PLATFORM".into()),
                Some("answered".into()),
                vec![],
                Some(signature),
                Some(signer),
                Some(public_key),
            )
            .unwrap();
        let log = read_log(&store.log_path()).unwrap();
        let ack = log.iter().find(|event| event.id > need.id).unwrap();
        match &ack.op {
            Op::Action { kind, outcome, .. } => {
                assert_eq!(kind, "ack");
                assert_eq!(outcome.as_deref(), Some("answered"));
            }
            other => panic!("expected the ack, got {other:?}"),
        }
        let open = store.messages(None, Some("PLATFORM"), None, false).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, need.id);
        assert_eq!(open[0].kind, "need");
        assert!(open[0].thread.contains(&sent[0].id));
        let closed = store
            .messages(None, Some("FOUNDER"), None, true)
            .unwrap()
            .into_iter()
            .find(|row| row.id == sent[0].id)
            .unwrap();
        assert_eq!(closed.status, "answered");
    }

    #[test]
    fn ping_is_only_the_orchestrator() {
        let (dir, store) = fixture();
        let path = plan_file(dir.path(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-15".into(),
                path,
                "example-platform".into(),
                vec![],
                "Example database".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let spoke = DeskNeed {
            for_founder: true,
            link: vec!["example-plans/setup.md".into()],
            ..DeskNeed::default()
        };
        let err = store
            .tell(
                1,
                "APP".into(),
                Some("LEAD".into()),
                false,
                "need".into(),
                "ask the operator".into(),
                vec![],
                vec![],
                spoke,
            )
            .unwrap_err();
        assert!(err.to_string().contains("orchestrator"), "{err}");
        let spoke = DeskNeed {
            for_founder: true,
            link: vec!["example-plans/setup.md".into()],
            ..DeskNeed::default()
        };
        let sent = store
            .tell(
                1,
                "APP".into(),
                Some("PLATFORM".into()),
                false,
                "need".into(),
                "ask the operator".into(),
                vec![],
                vec![],
                spoke,
            )
            .unwrap();
        let id = sent[0].id;
        let wrong = DeskNeed {
            ping: true,
            link: vec!["example-plans/orchestrator.md".into()],
            via: vec![id],
            ..DeskNeed::default()
        };
        let err = store
            .tell(
                1,
                "LEAD".into(),
                Some("FOUNDER".into()),
                false,
                "need".into(),
                "ping".into(),
                vec![],
                vec![],
                wrong,
            )
            .unwrap_err();
        assert!(err.to_string().contains("orchestrator"), "{err}");
        let ping = DeskNeed {
            ping: true,
            link: vec!["example-plans/orchestrator.md#ping".into()],
            via: vec![id],
            ..DeskNeed::default()
        };
        store
            .tell(
                1,
                "PLATFORM".into(),
                Some("FOUNDER".into()),
                false,
                "need".into(),
                "ping".into(),
                vec![],
                vec![],
                ping,
            )
            .unwrap();
        let secret = DeskNeed {
            ping: true,
            link: vec!["notes/see-ghp_EXAMPLETOKENVALUEHERE".into()],
            ..DeskNeed::default()
        };
        assert!(store
            .tell(
                1,
                "PLATFORM".into(),
                Some("FOUNDER".into()),
                false,
                "need".into(),
                "no".into(),
                vec![],
                vec![],
                secret,
            )
            .is_err());
        for bad in [
            "https://example.com/a",
            "plans/../secret.md",
            "not a link",
            "notes/gho_EXAMPLETOKEN",
            "notes/github_pat_EXAMPLE",
        ] {
            let desk = DeskNeed {
                ping: true,
                link: vec![bad.into()],
                ..DeskNeed::default()
            };
            assert!(
                store
                    .tell(
                        1,
                        "PLATFORM".into(),
                        Some("FOUNDER".into()),
                        false,
                        "need".into(),
                        "no".into(),
                        vec![],
                        vec![],
                        desk,
                    )
                    .is_err(),
                "{bad}"
            );
        }
        let rows = store.messages(None, Some("FOUNDER"), None, true).unwrap();
        let pinged: Vec<_> = rows.into_iter().filter(|row| row.desk.ping).collect();
        assert_eq!(pinged.len(), 1);
        assert_eq!(pinged[0].desk.via, vec![id]);
    }

    #[test]
    fn an_unsigned_founder_answer_stays_a_question() {
        let (_dir, store) = signer_fixture();
        let path = plan_file(store.root(), "plan.md", "body");
        store
            .plan_open(
                "PLATFORM-24".into(),
                path,
                "example-platform".into(),
                vec![],
                "Desk signer".into(),
                vec![],
                vec![],
            )
            .unwrap();
        let sent = store
            .tell(
                1,
                "RUN".into(),
                Some("FOUNDER".into()),
                false,
                "need".into(),
                "Which tier?".into(),
                vec![],
                vec![],
                DeskNeed::default(),
            )
            .unwrap();
        let id = sent[0].id;
        let planted = store.root().join("signers");
        std::fs::create_dir_all(&planted).unwrap();
        let (signature, signer, public_key) = sign_founder(id, "ack", "Essentials", "", "did");
        std::fs::write(
            planted.join("decoy.md"),
            format!("| Fingerprint | `{signer}` |\n"),
        )
        .unwrap();
        let err = store
            .answer(
                id,
                "FOUNDER".into(),
                "ack".into(),
                "Essentials".into(),
                None,
                Some("did".into()),
                vec![],
                Some(signature),
                Some(signer),
                Some(public_key),
            )
            .unwrap_err();
        assert!(err.to_string().contains("unsigned"), "{err}");
        record_signer(store.root());
        let err = store
            .answer(
                id,
                "FOUNDER".into(),
                "ack".into(),
                "Essentials".into(),
                None,
                Some("did".into()),
                vec![],
                None,
                None,
                None,
            )
            .unwrap_err();
        assert!(err.to_string().contains("unsigned"), "{err}");
        assert_eq!(store.messages(None, Some("FOUNDER"), None, false).unwrap().len(), 1);
        let pin = fingerprint_of(&test_public_raw());
        let err = store
            .answer(
                id,
                "FOUNDER".into(),
                "ack".into(),
                "Essentials".into(),
                None,
                Some("did".into()),
                vec![],
                Some("c2ln".into()),
                Some(pin.clone()),
                Some(base64::engine::general_purpose::STANDARD.encode(test_public_raw())),
            )
            .unwrap_err();
        assert!(err.to_string().contains("unsigned"), "{err}");
        let (signature, signer, public_key) = sign_founder(id, "ack", "Essentials", "", "did");
        let mut other = test_public_raw();
        other[1] ^= 0xff;
        let err = store
            .answer(
                id,
                "FOUNDER".into(),
                "ack".into(),
                "Essentials".into(),
                None,
                Some("did".into()),
                vec![],
                Some(signature.clone()),
                Some(fingerprint_of(&other)),
                Some(base64::engine::general_purpose::STANDARD.encode(other)),
            )
            .unwrap_err();
        assert!(err.to_string().contains("unsigned"), "{err}");
        let event = store
            .answer(
                id,
                "FOUNDER".into(),
                "ack".into(),
                "Essentials".into(),
                None,
                Some("did".into()),
                vec![],
                Some(signature.clone()),
                Some(signer),
                Some(public_key),
            )
            .unwrap();
        match event.op {
            Op::Action { desk, .. } => {
                assert_eq!(desk.signature.as_deref(), Some(signature.as_str()));
                assert_eq!(desk.signer.as_deref(), Some(pin.as_str()));
            }
            _ => panic!("not an action"),
        }
        assert!(store.messages(None, Some("FOUNDER"), None, false).unwrap().is_empty());
    }

    #[test]
    fn the_signed_event_matches_the_desk() {
        assert_eq!(
            answer_event(1501, "ack", "small tier, no reader", "PLATFORM", "did"),
            r#"{"id":1501,"agent":"FOUNDER","kind":"ack","body":"small tier, no reader","to":"PLATFORM","outcome":"did"}"#
        );
        assert_eq!(
            answer_event(9, "ack", "Essentials", "", "did"),
            r#"{"id":9,"agent":"FOUNDER","kind":"ack","body":"Essentials","outcome":"did"}"#
        );
        assert_eq!(
            answer_event(9, "tell", "a & b <c>", "PLATFORM", "moved"),
            r#"{"id":9,"agent":"FOUNDER","kind":"tell","body":"a \u0026 b \u003cc\u003e","to":"PLATFORM","outcome":"moved"}"#
        );
    }
}
