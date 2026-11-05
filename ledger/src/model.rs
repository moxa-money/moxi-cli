use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const SUMMARY_MAX: usize = 120;
pub const BODY_MAX: usize = 400;
/// Notice board. A channel post is addressed here. NB is not an agent and does not listen.
pub const NOTICE_BOARD: &str = "NB";
/// Human operator. This build uses one local name. A worker name comes from owners.toml.
pub const OPERATOR: &str = "FOUNDER";
pub const STATUSES: &[&str] = &["open", "blocked", "pinned", "closed", "cancelled"];
pub const DESK_CATEGORIES: &[&str] = &[
    "security", "cost", "vendor", "product", "privacy", "prod", "process",
];
/// Dispatcher profile. Separate from the desk category list.
pub const PROFILES: &[&str] = &["review", "build", "routine"];

pub const ACTION_KINDS: &[&str] = &[
    "note",
    "handoff",
    "block",
    "unblock",
    "pr",
    "merge",
    "close",
    "pin",
    "worktree",
    "supersede",
    "need",
    "tell",
    "notice",
    "ack",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    PlanOpen {
        plan_ref: String,
        path: String,
        content_hash: String,
        repo: String,
        depends_on: Vec<String>,
        summary: String,
        tags: Vec<String>,
        entities: Vec<String>,
    },
    PlanUpdate {
        plan_ref: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depends_on: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tags: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entities: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    ItemOpen {
        plan_ref: String,
        repo: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        requesting_agent: String,
        requested_agent: String,
        summary: String,
        tags: Vec<String>,
        entities: Vec<String>,
    },
    Action {
        record_id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_id: Option<u64>,
        kind: String,
        body: String,
        mentions: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocked_by: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        clear_blocked_by: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_agent: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pr_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        merge_sha: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        merge_at: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        clear_worktree: bool,
        /// Agent who superseded the work. Set only on a supersede action.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
        /// Plan ref or action id the work pivoted to. Set only on a supersede action.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pivot: Option<String>,
        /// Recipient. A channel post is NB. A need, a tell, and a handoff name the agent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
        /// Answer on an ack: did, later, ask-founder, or ackn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<String>,
        /// Message ids this message addresses. A reply includes the message it answers.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        thread: Vec<u64>,
        /// Desk fields on a need, and the signature on an answer. Empty on older events.
        #[serde(flatten, default, skip_serializing_if = "DeskNeed::is_empty")]
        desk: DeskNeed,
    },
    /// Record which messages an existing message addresses. This does not send mail.
    Thread {
        message_id: u64,
        thread: Vec<u64>,
        agent: String,
    },
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Fields a need to FOUNDER may carry, and the signature on a desk answer.
/// Every field defaults to empty so an older event still loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DeskNeed {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Dispatcher profile: review, build, or routine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub option: Vec<DeskOption>,
    /// How the agent will check a step the operator has to do by hand. Present means the step is theirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_step: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub link: Vec<String>,
    /// Orchestrator ping to FOUNDER. A spoke cannot set this.
    #[serde(default, skip_serializing_if = "is_false")]
    pub ping: bool,
    /// Spoke need for the operator. It goes to the plan's orchestrator.
    #[serde(default, skip_serializing_if = "is_false")]
    pub for_founder: bool,
    /// Spoke needs this ping rolls up.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data: Vec<DeskDatum>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    /// Queue kind stored on the need. The desk does not infer it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_kind: Option<String>,
}

impl DeskNeed {
    pub fn is_empty(&self) -> bool {
        self.urgency.is_none()
            && self.category.is_none()
            && self.profile.is_none()
            && self.title.is_none()
            && self.option.is_empty()
            && self.human_step.is_none()
            && self.action.is_empty()
            && self.why.is_none()
            && self.standard.is_none()
            && self.link.is_empty()
            && !self.ping
            && !self.for_founder
            && self.via.is_empty()
            && self.data.is_empty()
            && self.signature.is_none()
            && self.signer.is_none()
            && self.queue_kind.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeskOption {
    pub label: String,
    pub allows: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeskDatum {
    pub label: String,
    pub number: f64,
}

/// Ops this binary applies. A later op is skipped so a listener keeps running.
pub fn known_op(name: &str) -> bool {
    matches!(
        name,
        "plan_open" | "plan_update" | "item_open" | "action" | "thread"
    )
}

fn link_carries_secret(link: &str) -> bool {
    let lower = link.to_ascii_lowercase();
    lower.contains("begin ")
        || lower.contains("private key")
        || link.contains("AKIA")
        || lower.contains("ghp_")
        || lower.contains("gho_")
        || lower.contains("ghs_")
        || lower.contains("github_pat_")
        || lower.contains("sk-")
}

fn link_shape(link: &str) -> Result<()> {
    if link.is_empty() || link.chars().any(|c| c.is_whitespace()) || link.contains("://") {
        bail!("a link is a record id, a path, or a canvas section");
    }
    if link_carries_secret(link) {
        bail!("a link cannot carry a secret");
    }
    let (path, section) = match link.split_once('#') {
        Some((path, section)) => {
            if path.is_empty() || section.is_empty() || section.contains('#') {
                bail!("a link is a record id, a path, or a canvas section");
            }
            (path, true)
        }
        None => (link, false),
    };
    if !section && path.chars().all(|c| c.is_ascii_digit()) {
        return Ok(());
    }
    let segments: Vec<&str> = path.split('/').collect();
    for (index, segment) in segments.iter().enumerate() {
        if segment.is_empty() && index == 0 && path.starts_with('/') {
            continue;
        }
        if segment.is_empty() || *segment == "." || *segment == ".." {
            bail!("a link cannot contain a .. segment");
        }
    }
    Ok(())
}

pub fn check_desk(desk: &DeskNeed) -> Result<()> {
    if let Some(urgency) = desk.urgency {
        if !(1..=3).contains(&urgency) {
            bail!("urgency is 1, 2, or 3");
        }
    }
    if let Some(category) = &desk.category {
        if !DESK_CATEGORIES.contains(&category.as_str()) {
            bail!("category must be security, cost, vendor, product, privacy, prod, or process");
        }
    }
    if let Some(profile) = &desk.profile {
        if !PROFILES.contains(&profile.as_str()) {
            bail!("profile must be review, build, or routine");
        }
    }
    if let Some(title) = &desk.title {
        check_line("title", title, 60)?;
    }
    if desk.option.len() > 9 {
        bail!("a request has at most 9 options");
    }
    for option in &desk.option {
        check_line("option", &option.label, 200)?;
        check_line("allows", &option.allows, 200)?;
        if let Some(cost) = &option.cost {
            check_line("cost", cost, 80)?;
        }
        if let Some(risk) = &option.risk {
            check_line("risk", risk, 200)?;
        }
    }
    if let Some(step) = &desk.human_step {
        check_line("human_step", step, 200)?;
    }
    if desk.action.len() > 3 {
        bail!("a request has at most 3 quick actions");
    }
    for action in &desk.action {
        check_line("action", action, 200)?;
    }
    if let Some(why) = &desk.why {
        check_line("why", why, 200)?;
    }
    if let Some(standard) = &desk.standard {
        check_line("standard", standard, 400)?;
    }
    for link in &desk.link {
        check_line("link", link, 400)?;
    }
    if desk.ping && desk.for_founder {
        bail!("a need is --for-founder or --ping");
    }
    if (desk.ping || desk.for_founder) && desk.link.is_empty() {
        bail!("--for-founder and --ping need a link");
    }
    if desk.ping || desk.for_founder {
        for link in &desk.link {
            link_shape(link)?;
        }
    }
    if !desk.via.is_empty() && !desk.ping {
        bail!("--via rolls up a ping");
    }
    let mut seen_via = std::collections::BTreeSet::new();
    for id in &desk.via {
        if *id == 0 || !seen_via.insert(id) {
            bail!("via lists each spoke need once");
        }
    }
    if desk.data.len() > 8 {
        bail!("a request chart has at most 8 numbers");
    }
    for datum in &desk.data {
        check_line("data", &datum.label, 80)?;
    }
    if let Some(signature) = &desk.signature {
        check_line("signature", signature, 400)?;
    }
    if let Some(signer) = &desk.signer {
        check_line("signer", signer, 200)?;
    }
    if let Some(kind) = &desk.queue_kind {
        if !matches!(
            kind.as_str(),
            "Sign in" | "Decide" | "Approve a plan" | "Supply" | "Test" | "Machine"
        ) {
            bail!("queue kind is Sign in, Decide, Approve a plan, Supply, Test, or Machine");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub v: u32,
    pub id: u64,
    pub at: String,
    #[serde(flatten)]
    pub op: Op,
}

#[derive(Debug, Clone, Serialize)]
pub struct Record {
    pub id: u64,
    pub kind: String,
    pub plan_ref: Option<String>,
    pub plan_id: Option<u64>,
    pub repo: String,
    pub path: Option<String>,
    pub content_hash: Option<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub requesting_agent: Option<String>,
    pub requested_agent: Option<String>,
    pub summary: String,
    pub tags: Vec<String>,
    pub entities: Vec<String>,
    pub status: String,
    pub depends_on: Vec<String>,
    pub blocked_by: Option<String>,
    pub pr_id: Option<String>,
    pub merge_sha: Option<String>,
    pub merge_at: Option<String>,
    pub opened_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Action {
    pub id: u64,
    pub record_id: u64,
    pub parent_id: Option<u64>,
    pub at: String,
    pub kind: String,
    pub body: String,
    pub mentions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pivot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thread: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnedRecord {
    #[serde(flatten)]
    pub record: Record,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Show {
    #[serde(flatten)]
    pub record: Record,
    pub owner: String,
    pub items: Vec<OwnedRecord>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: u64,
    pub record_id: u64,
    pub plan_ref: Option<String>,
    pub kind: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub at: String,
    pub status: String,
    pub body: String,
    /// True when this tell is a reply to a request. ackn does not close it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub answer_tell: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Message ids this message addresses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thread: Vec<u64>,
    /// Desk fields. Empty on a notice that is not a request.
    #[serde(flatten, default, skip_serializing_if = "DeskNeed::is_empty")]
    pub desk: DeskNeed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub id: u64,
    pub record_id: u64,
    pub plan_ref: Option<String>,
    pub kind: String,
    pub owner: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadyPlan {
    #[serde(flatten)]
    pub record: Record,
    pub owner: String,
    pub items: Vec<OwnedRecord>,
}

pub fn check_op(op: &Op) -> Result<()> {
    check_op_mode(op, true)
}

/// Shape checks for replay and rebuild. Secret detectors run only on a new write.
pub fn check_op_replay(op: &Op) -> Result<()> {
    check_op_mode(op, false)
}

fn check_op_mode(op: &Op, secrets: bool) -> Result<()> {
    match op {
        Op::PlanOpen {
            plan_ref,
            path,
            repo,
            depends_on,
            summary,
            tags,
            entities,
            ..
        } => {
            check_ref(plan_ref)?;
            check_line("path", path, 400)?;
            check_repo_name(repo)?;
            for dep in depends_on {
                check_ref(dep)?;
            }
            check_summary_mode(summary, secrets)?;
            check_tokens("tag", tags, 32)?;
            check_tokens("entity", entities, 80)?;
        }
        Op::PlanUpdate {
            plan_ref,
            path,
            depends_on,
            summary,
            tags,
            entities,
            status,
            ..
        } => {
            check_ref(plan_ref)?;
            if let Some(path) = path {
                check_line("path", path, 400)?;
            }
            if let Some(deps) = depends_on {
                for dep in deps {
                    check_ref(dep)?;
                    if dep == plan_ref {
                        bail!("plan {plan_ref} cannot depend on itself");
                    }
                }
            }
            if let Some(summary) = summary {
                check_summary_mode(summary, secrets)?;
            }
            if let Some(tags) = tags {
                check_tokens("tag", tags, 32)?;
            }
            if let Some(entities) = entities {
                check_tokens("entity", entities, 80)?;
            }
            if let Some(status) = status {
                check_status(status)?;
            }
        }
        Op::ItemOpen {
            plan_ref,
            repo,
            requesting_agent,
            requested_agent,
            summary,
            tags,
            entities,
            worktree,
            branch,
        } => {
            check_ref(plan_ref)?;
            check_repo_name(repo)?;
            check_agent("requesting agent", requesting_agent)?;
            check_agent("requested agent", requested_agent)?;
            check_summary_mode(summary, secrets)?;
            check_tokens("tag", tags, 32)?;
            check_tokens("entity", entities, 80)?;
            if let Some(worktree) = worktree {
                check_line("worktree", worktree, 400)?;
            }
            if let Some(branch) = branch {
                check_line("branch", branch, 200)?;
            }
        }
        Op::Action {
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
            record_id: _,
            parent_id: _,
        } => {
            let mut seen = std::collections::BTreeSet::new();
            for id in thread {
                if *id == 0 || !seen.insert(id) {
                    bail!("thread lists each message id once");
                }
            }
            if !ACTION_KINDS.contains(&kind.as_str()) {
                bail!("unknown action kind {kind}");
            }
            check_body_mode(body, secrets)?;
            check_tokens("mention", mentions, 200)?;
            if let Some(status) = status {
                check_status(status)?;
            }
            if let Some(agent) = requested_agent {
                check_agent("requested agent", agent)?;
            }
            if let Some(pr) = pr_id {
                check_line("pr", pr, 200)?;
            }
            if let Some(sha) = merge_sha {
                check_sha(sha)?;
            }
            if let Some(at) = merge_at {
                check_time(at)?;
            }
            if let Some(worktree) = worktree {
                check_line("worktree", worktree, 400)?;
            }
            if let Some(branch) = branch {
                check_line("branch", branch, 200)?;
            }
            if let Some(value) = outcome.as_deref() {
                if !known_outcome(value) {
                    bail!("unknown outcome");
                }
            }
            match kind.as_str() {
                "block" => {
                    if blocked_by.as_deref().unwrap_or("").is_empty() {
                        bail!("block requires --by");
                    }
                    if *clear_blocked_by {
                        bail!("block cannot clear blocked_by");
                    }
                }
                "unblock" => {
                    if !*clear_blocked_by {
                        bail!("unblock clears blocked_by");
                    }
                }
                "handoff" => {
                    if requested_agent.is_none() {
                        bail!("handoff requires --requested");
                    }
                }
                "pr" => {
                    if pr_id.is_none() {
                        bail!("pr requires --pr");
                    }
                }
                "merge" => {
                    if merge_sha.is_none() {
                        bail!("merge requires --sha");
                    }
                }
                "worktree" => {
                    if !*clear_worktree && worktree.is_none() {
                        bail!("worktree requires --path or --clear");
                    }
                }
                "need" | "tell" => {
                    let Some(agent) = agent else {
                        bail!("{kind} requires --agent");
                    };
                    check_agent("agent", agent)?;
                    let Some(recipient) = to else {
                        bail!("{kind} requires a recipient");
                    };
                    check_agent("recipient", recipient)?;
                }
                "notice" => {
                    let Some(agent) = agent else {
                        bail!("notice requires --agent");
                    };
                    check_agent("agent", agent)?;
                    if let Some(recipient) = to {
                        check_agent("recipient", recipient)?;
                    }
                }
                "ack" => {
                    let Some(agent) = agent else {
                        bail!("ack requires --agent");
                    };
                    check_agent("agent", agent)?;
                    match outcome.as_deref() {
                        Some(
                            "did"
                            | "done"
                            | "later"
                            | "ask-founder"
                            | "to_founder"
                            | "ackn"
                            | "answered"
                            | "withdrawn",
                        ) => {}
                        _ => bail!("an ack outcome is did, done, later, to_founder, ackn, answered, or withdrawn"),
                    }
                }
                "supersede" => {
                    let Some(agent) = agent else {
                        bail!("supersede requires --agent");
                    };
                    check_agent("agent", agent)?;
                    let Some(pivot) = pivot else {
                        bail!("supersede requires --by");
                    };
                    check_line("pivot", pivot, 120)?;
                    if !*clear_blocked_by {
                        bail!("supersede clears blocked_by");
                    }
                    if status.as_deref() != Some("cancelled") {
                        bail!("supersede sets status cancelled");
                    }
                }
                _ => {}
            }
            check_desk(desk)?;
        }
        Op::Thread {
            message_id,
            thread,
            agent,
        } => {
            if *message_id == 0 {
                bail!("thread needs a message id");
            }
            check_agent("agent", agent)?;
            if thread.is_empty() {
                bail!("thread lists at least one message id");
            }
            let mut seen = std::collections::BTreeSet::new();
            for id in thread {
                if *id == 0 || *id == *message_id || !seen.insert(*id) {
                    bail!("thread lists each other message id once");
                }
            }
        }
    }
    Ok(())
}

pub fn check_summary(summary: &str) -> Result<()> {
    check_summary_mode(summary, true)
}

fn check_summary_mode(summary: &str, secrets: bool) -> Result<()> {
    if secrets {
        refuse_ledger_secret("summary", summary)?;
    }
    check_line("summary", summary, SUMMARY_MAX)
}

pub fn check_body(body: &str) -> Result<()> {
    check_body_mode(body, true)
}

fn check_body_mode(body: &str, secrets: bool) -> Result<()> {
    if secrets {
        refuse_ledger_secret("body", body)?;
    }
    check_line("body", body, BODY_MAX)
}

/// Block the settled secret detectors. Email and a Luhn number stay, matching the notes refusal.
fn refuse_ledger_secret(label: &str, text: &str) -> Result<()> {
    if let Some(detector) = crate::notes::secret_detector(text) {
        bail!("{label} was refused ({detector}); use desk runbook secret.put");
    }
    Ok(())
}

/// Outcomes the ledger accepts. `moved`, `did`, and `ask-founder` stay readable.
pub fn known_outcome(outcome: &str) -> bool {
    matches!(
        outcome,
        "did"
            | "done"
            | "later"
            | "ask-founder"
            | "to_founder"
            | "ackn"
            | "answered"
            | "withdrawn"
            | "moved"
    )
}

pub fn check_status(status: &str) -> Result<()> {
    if STATUSES.contains(&status) {
        Ok(())
    } else {
        bail!("status must be open, blocked, pinned, closed, or cancelled")
    }
}

pub fn check_agent(label: &str, agent: &str) -> Result<()> {
    let trimmed = agent.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > 32
        || trimmed.chars().any(|c| c.is_whitespace())
    {
        bail!("{label} must be one word, up to 32 characters");
    }
    Ok(())
}

pub fn check_owner(agent: &str) -> Result<()> {
    check_agent("repo owner", agent)
}

pub fn check_ref(plan_ref: &str) -> Result<()> {
    check_line("plan", plan_ref, 120)
}

fn check_repo_name(repo: &str) -> Result<()> {
    check_line("repo", repo, 80)
}

fn check_tokens(label: &str, values: &[String], max: usize) -> Result<()> {
    if values.len() > 16 {
        bail!("too many {label}s");
    }
    for value in values {
        check_line(label, value, max)?;
    }
    Ok(())
}

fn check_line(label: &str, value: &str, max: usize) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{label} is empty");
    }
    if trimmed.chars().count() > max {
        bail!("{label} is longer than {max} characters");
    }
    if trimmed.chars().any(|c| c == '\n' || c == '\r') {
        bail!("{label} must be one line");
    }
    Ok(())
}

fn check_sha(sha: &str) -> Result<()> {
    let n = sha.len();
    if !(7..=64).contains(&n) || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("merge sha must be 7 to 64 hex characters");
    }
    Ok(())
}

pub fn check_time(value: &str) -> Result<()> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|_| anyhow::anyhow!("time must be RFC3339"))?;
    Ok(())
}

pub fn hash_file(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{digest:x}"))
}

pub fn now_utc() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_json_omits_author() {
        let event = Event {
            v: 1,
            id: 1,
            at: "2026-10-03T01:00:00Z".into(),
            op: Op::Action {
                record_id: 1,
                parent_id: None,
                kind: "note".into(),
                body: "example database still on schema step 1".into(),
                mentions: vec!["PLATFORM-15".into()],
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
},
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("author"));
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn queue_kind_is_stored_and_a_missing_or_unknown_kind_is_refused() {
        let ok = DeskNeed {
            queue_kind: Some("Supply".into()),
            ..DeskNeed::default()
        };
        check_desk(&ok).unwrap();
        let unknown = DeskNeed {
            queue_kind: Some("Later".into()),
            ..DeskNeed::default()
        };
        let err = check_desk(&unknown).unwrap_err().to_string();
        assert!(err.contains("queue kind"), "{err}");
        let missing = DeskNeed::default();
        check_desk(&missing).unwrap();
        assert!(missing.queue_kind.is_none());
    }

    #[test]
    fn profile_is_review_build_or_routine_and_not_a_category() {
        let ok = DeskNeed {
            profile: Some("routine".into()),
            category: Some("process".into()),
            ..DeskNeed::default()
        };
        check_desk(&ok).unwrap();
        let err = check_desk(&DeskNeed {
            profile: Some("cost".into()),
            ..DeskNeed::default()
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("profile"), "{err}");
    }

    #[test]
    fn ledger_text_refuses_secret_detectors_and_keeps_flag_only() {
        let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/notes-secret-refusal.json"));
        let raw = std::fs::read_to_string(path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
        for case in doc["cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let text = case["text"].as_str().unwrap();
            let expect = case["expect"].as_str().unwrap();
            if expect == "refuse" {
                let detector = crate::notes::secret_detector(text).expect(id);
                let err = check_body(text).unwrap_err().to_string();
                assert!(err.contains(detector), "{id}: {err}");
                assert!(err.contains("secret.put"), "{id}: {err}");
                assert!(!err.contains(text), "{id} echoed the body");
                if text.chars().count() <= SUMMARY_MAX && !text.contains('\n') {
                    let summary_err = check_summary(text).unwrap_err().to_string();
                    assert!(summary_err.contains(detector), "{id}: {summary_err}");
                    assert!(!summary_err.contains(text), "{id} echoed the summary");
                }
            } else {
                check_body(text).unwrap_or_else(|err| panic!("{id} refused: {err}"));
                if text.chars().count() <= SUMMARY_MAX {
                    check_summary(text).unwrap_or_else(|err| panic!("{id} summary refused: {err}"));
                }
            }
        }
    }

    fn notice_op(body: &str) -> Op {
        Op::Action {
            record_id: 1,
            parent_id: None,
            kind: "notice".into(),
            body: body.into(),
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
            agent: Some("RUN".into()),
            pivot: None,
            to: Some(NOTICE_BOARD.into()),
            outcome: None,
            thread: vec![],
            desk: DeskNeed::default(),
        }
    }

    #[test]
    fn replay_skips_secret_detectors_and_keeps_shape() {
        let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/notes-secret-refusal.json"));
        let raw = std::fs::read_to_string(path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let text = doc["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == "refuse-aws-key-id")
            .unwrap()["text"]
            .as_str()
            .unwrap();
        let op = notice_op(text);
        let err = check_op(&op).unwrap_err().to_string();
        assert!(err.contains("aws_key_id"), "{err}");
        assert!(err.contains("secret.put"), "{err}");
        assert!(!err.contains(text));
        check_op_replay(&op).unwrap();
        let plan = Op::PlanOpen {
            plan_ref: "IG-1".into(),
            path: "plan.md".into(),
            content_hash: "abc".into(),
            repo: "ledger".into(),
            depends_on: vec![],
            summary: text.into(),
            tags: vec![],
            entities: vec![],
        };
        assert!(check_op(&plan).is_err());
        check_op_replay(&plan).unwrap();
        let shape = check_op_replay(&notice_op(&"x".repeat(BODY_MAX + 1)))
            .unwrap_err()
            .to_string();
        assert!(shape.contains("longer than"), "{shape}");
    }
}
