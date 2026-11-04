use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use comfy_table::{presets, Table};
use comms::model::{
    check_agent, check_status, hash_file, Action, DeskDatum, DeskNeed, DeskOption, Op,
    OwnedRecord, ReadyPlan, Show,
};
use comms::resolve_root;
use comms::store::Store;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "comms",
    version,
    about = "Founder-local coordination ledger. Workers are named in owners.toml. SQLite is bundled in this binary."
)]
struct Cli {
    /// ledger root. Defaults to COMMS_DIR, then a walk from the current directory.
    #[arg(long, global = true)]
    dir: Option<PathBuf>,
    /// Print JSON for agents.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create the data directory and an empty log.
    Init,
    /// Show repo owners. The owner is the servicing agent.
    Owners,
    #[command(subcommand)]
    Plan(PlanCmd),
    #[command(subcommand)]
    Item(ItemCmd),
    /// Add a note to a record.
    Note(NoteArgs),
    /// Idea store outside the ledger. A promotion writes a fresh summary only.
    #[command(subcommand)]
    Notes(NotesCmd),
    /// Mark a record blocked on a plan ref.
    Block(BlockArgs),
    /// Clear a block and set the record open.
    Unblock(BodyArgs),
    /// Cancel a record as replaced, not finished. Unfinished items on a plan are cancelled too.
    Supersede(SupersedeArgs),
    /// Save a record for later.
    Pin(BodyArgs),
    /// Close a record. Merge fields stay empty unless you used merge.
    /// A plan stays open while another record is blocked by it, until each has a note or an unblock that names this plan.
    Close(BodyArgs),
    /// Set the agent who must act next and send them a notice.
    Handoff(HandoffArgs),
    /// Send a fact or a need. --all writes one notice to each agent with open work on that plan, except the sender. It does not include FOUNDER.
    Tell(TellArgs),
    /// Post to the channel. No acknowledgement. Follow shows it.
    Notice(NoticeArgs),
    /// Answer a notice and send that text back to its sender.
    Reply(ReplyArgs),
    /// Write one desk answer. The kind is the only event written.
    Answer(AnswerArgs),
    /// Answer a notice. Only the recipient may answer.
    Ack(AckArgs),
    /// The sender cancels their own open need or tell.
    Withdraw(WithdrawArgs),
    /// Record which messages an existing message addresses. This does not send mail.
    Thread(ThreadArgs),
    /// Flat list of open notices, oldest first. --all lists every notice.
    Messages(MessagesArgs),
    /// Record a repo-qualified pull request id, such as example-app#24.
    Pr(PrArgs),
    /// Record the merge commit and close the record.
    /// A plan stays open while another record is blocked by it, until each has a note or an unblock that names this plan.
    Merge(MergeArgs),
    /// Set or clear the git worktree on an item.
    Worktree(WorktreeArgs),
    /// List records by id. Open and blocked are the default. This is not a queue.
    List(ListArgs),
    /// Plans that are open and whose depends_on plans are all closed.
    Ready,
    /// Show a record by numeric id or plan ref.
    Show { key: String },
    /// Records that mention a plan ref, depend on it, or are blocked by it.
    Related { key: String },
    /// Full-text search across summaries, tags, entities, and action text.
    Search { words: Vec<String> },
    /// Print the action tree for a record.
    Tree { id: u64 },
    /// Follow the ledger and print a wake line when this agent's open inbox changes.
    Listen(ListenArgs),
    /// Rebuild the SQLite projection from events.jsonl.
    Rebuild,
}

#[derive(Subcommand)]
enum PlanCmd {
    Open(PlanOpenArgs),
    Update(PlanUpdateArgs),
}

#[derive(Subcommand)]
enum ItemCmd {
    Open(ItemOpenArgs),
}

#[derive(Args)]
struct PlanOpenArgs {
    #[arg(long)]
    plan_ref: String,
    #[arg(long)]
    repo: String,
    #[arg(long)]
    path: PathBuf,
    #[arg(long)]
    summary: String,
    #[arg(long)]
    depends_on: Vec<String>,
    #[arg(long)]
    tag: Vec<String>,
    #[arg(long)]
    entity: Vec<String>,
}

#[derive(Args)]
struct PlanUpdateArgs {
    #[arg(long)]
    plan_ref: String,
    #[arg(long)]
    path: Option<PathBuf>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    depends_on: Vec<String>,
    #[arg(long)]
    clear_depends: bool,
    #[arg(long)]
    tag: Vec<String>,
    #[arg(long)]
    clear_tags: bool,
    #[arg(long)]
    entity: Vec<String>,
    #[arg(long)]
    clear_entities: bool,
}

#[derive(Args)]
struct ItemOpenArgs {
    #[arg(long)]
    plan: String,
    #[arg(long)]
    repo: String,
    #[arg(long)]
    requesting: String,
    #[arg(long)]
    requested: String,
    #[arg(long)]
    summary: String,
    #[arg(long)]
    worktree: Option<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long)]
    tag: Vec<String>,
    #[arg(long)]
    entity: Vec<String>,
}

#[derive(Args)]
struct SupersedeArgs {
    #[arg(long)]
    id: u64,
    /// Agent who is cancelling the work.
    #[arg(long)]
    agent: String,
    /// Plan ref or action id the work pivots to.
    #[arg(long)]
    by: String,
    /// Why this work will not be finished.
    #[arg(long)]
    body: String,
    #[arg(long)]
    mention: Vec<String>,
}

#[derive(Subcommand)]
enum NotesCmd {
    /// Write an idea. A secret-shaped body is refused.
    New(NotesNewArgs),
    /// List ideas.
    List,
    /// Show one idea.
    Show(NotesIdArgs),
    /// Write a fresh summary onto a plan. The note body stays out of the ledger.
    Promote(NotesPromoteArgs),
    /// Mark an idea dropped. The line stays in the file.
    Drop(NotesIdArgs),
}

#[derive(Args)]
struct NotesNewArgs {
    #[arg(long)]
    author: String,
    #[arg(long)]
    title: String,
    #[arg(long)]
    body: String,
    #[arg(long)]
    tag: Vec<String>,
    #[arg(long)]
    source: Option<String>,
}

#[derive(Args)]
struct NotesIdArgs {
    #[arg(long)]
    id: u64,
}

#[derive(Args)]
struct NotesPromoteArgs {
    #[arg(long)]
    id: u64,
    /// Plan ref or record id that receives the fresh summary.
    #[arg(long)]
    on: String,
    #[arg(long)]
    agent: String,
    #[arg(long)]
    summary: String,
}

#[derive(Args)]
struct NoteArgs {
    #[arg(long)]
    id: u64,
    #[arg(long)]
    parent: Option<u64>,
    #[arg(long)]
    body: String,
    #[arg(long)]
    mention: Vec<String>,
}

#[derive(Args)]
struct BlockArgs {
    #[arg(long)]
    id: u64,
    #[arg(long)]
    by: String,
    #[arg(long)]
    parent: Option<u64>,
    #[arg(long)]
    body: String,
    #[arg(long)]
    mention: Vec<String>,
    #[arg(long)]
    requested: Option<String>,
}

#[derive(Args)]
struct BodyArgs {
    #[arg(long)]
    id: u64,
    #[arg(long)]
    parent: Option<u64>,
    #[arg(long)]
    body: Option<String>,
    #[arg(long)]
    mention: Vec<String>,
    /// Agent who is acting. An unblock or close with this set wakes the assignee.
    #[arg(long)]
    agent: Option<String>,
}

#[derive(Args)]
struct HandoffArgs {
    #[arg(long)]
    id: u64,
    /// Agent who is handing the work on.
    #[arg(long)]
    agent: String,
    #[arg(long)]
    requested: String,
    #[arg(long)]
    parent: Option<u64>,
    #[arg(long)]
    body: Option<String>,
    #[arg(long)]
    mention: Vec<String>,
}

#[derive(Args)]
struct TellArgs {
    /// Record id, plan ref, or NA when the notice is not about a project.
    #[arg(long)]
    on: String,
    /// Agent who is sending.
    #[arg(long)]
    agent: String,
    /// One recipient. Omit this when using --all.
    #[arg(long)]
    to: Option<String>,
    /// One notice to each agent with open or blocked work on that plan. The sender and FOUNDER are left out.
    #[arg(long)]
    all: bool,
    /// Ask for an answer. This is a need.
    #[arg(long)]
    need: bool,
    /// A fact to know. This is a tell, and it is the default.
    #[arg(long)]
    fyi: bool,
    #[arg(long)]
    body: String,
    #[arg(long)]
    mention: Vec<String>,
    #[arg(long)]
    urgency: Option<u8>,
    #[arg(long)]
    category: Option<String>,
    /// Dispatcher profile: review, build, or routine.
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    title: Option<String>,
    /// label|allows, with an optional cost and risk after more bars.
    #[arg(long = "option")]
    option_line: Vec<String>,
    #[arg(long = "human-step")]
    human_step: Option<String>,
    #[arg(long = "action")]
    action: Vec<String>,
    #[arg(long)]
    why: Option<String>,
    #[arg(long)]
    standard: Option<String>,
    /// Stored queue kind. Sign in, Decide, Approve a plan, Supply, Test, or Machine.
    #[arg(long = "queue-kind")]
    queue_kind: Option<String>,
    #[arg(long = "link")]
    link: Vec<String>,
    /// Spoke need for the operator. It goes to the plan's orchestrator, with a link.
    #[arg(long = "for-founder")]
    for_founder: bool,
    /// Orchestrator ping to FOUNDER. Refused unless the sender owns the plan.
    #[arg(long)]
    ping: bool,
    /// Spoke need this ping rolls up. Repeat for each.
    #[arg(long)]
    via: Vec<u64>,
    /// label|number for the request chart. Repeat for each.
    #[arg(long = "data")]
    data_line: Vec<String>,
    /// Message ids this message addresses. Repeat for each.
    #[arg(long)]
    thread: Vec<u64>,
}

#[derive(Args)]
struct NoticeArgs {
    /// Record id, plan ref, or NA when the post is not about a project.
    #[arg(long)]
    on: String,
    /// Agent who is posting.
    #[arg(long)]
    agent: String,
    /// Notice board. The only recipient is NB. Omit this and the post still goes to NB.
    #[arg(long)]
    to: Option<String>,
    #[arg(long)]
    body: String,
    #[arg(long)]
    mention: Vec<String>,
    /// Message ids this post addresses. Repeat for each.
    #[arg(long)]
    thread: Vec<u64>,
}

#[derive(Args)]
struct ReplyArgs {
    #[arg(long)]
    id: u64,
    #[arg(long)]
    agent: String,
    #[arg(long)]
    body: String,
    /// Other message ids this reply addresses. The notice itself is included.
    #[arg(long)]
    thread: Vec<u64>,
}

#[derive(Args)]
struct AnswerArgs {
    #[arg(long)]
    id: u64,
    #[arg(long)]
    agent: String,
    /// ack, tell, or need.
    #[arg(long)]
    kind: String,
    #[arg(long)]
    body: String,
    #[arg(long)]
    to: Option<String>,
    #[arg(long)]
    outcome: Option<String>,
    #[arg(long)]
    thread: Vec<u64>,
    /// Signature over the whole answer event. An empty signature stays a question.
    #[arg(long)]
    signature: Option<String>,
    /// Fingerprint of the key that signed the event.
    #[arg(long)]
    signer: Option<String>,
    /// Uncompressed P-256 public key, base64. Checked against the pin and not stored.
    #[arg(long)]
    public_key: Option<String>,
}

#[derive(Args)]
struct AckArgs {
    /// Notice id.
    #[arg(long)]
    id: u64,
    #[arg(long)]
    agent: String,
    #[arg(long)]
    did: Option<String>,
    #[arg(long)]
    later: Option<String>,
    #[arg(long = "ask-founder")]
    ask_founder: Option<String>,
    /// Acknowledge a tell and close it. A need cannot be acknowledged this way.
    #[arg(long, num_args = 0..=1, default_missing_value = "acknowledged")]
    ackn: Option<String>,
    /// Close a tell as answered. The asker still owns the next step.
    #[arg(long)]
    answered: Option<String>,
    /// Other message ids this answer addresses. The notice itself is included.
    #[arg(long)]
    thread: Vec<u64>,
}

#[derive(Args)]
struct WithdrawArgs {
    /// Notice id.
    #[arg(long)]
    id: u64,
    /// Sender of the notice. Only that agent may withdraw it.
    #[arg(long)]
    agent: String,
    #[arg(long)]
    body: String,
}

#[derive(Args)]
struct ThreadArgs {
    /// Message to annotate.
    #[arg(long)]
    id: u64,
    #[arg(long)]
    agent: String,
    /// Message id this message addresses. Repeat for each.
    #[arg(long)]
    thread: Vec<u64>,
}

#[derive(Args)]
struct ListenArgs {
    /// One word, up to 32 characters. One process per worker.
    #[arg(long)]
    agent: String,
    /// How often to read the inbox again, in seconds, even if the log is quiet.
    #[arg(long, default_value_t = 2)]
    refresh: u64,
    /// Wake when a need this agent sent is answered, as well as on new inbound mail.
    /// The plan owner uses this. A spoke does not.
    #[arg(long)]
    orch: bool,
}

#[derive(Args)]
struct MessagesArgs {
    /// Sender.
    #[arg(long)]
    from: Option<String>,
    /// Recipient.
    #[arg(long)]
    to: Option<String>,
    /// Record id or plan ref.
    #[arg(long)]
    record: Option<String>,
    /// Every notice. The default is open notices only, oldest first.
    #[arg(long)]
    all: bool,
    /// Include channel posts. Follow uses this. A channel post is not inbox mail.
    #[arg(long)]
    channel: bool,
    /// Newest channel posts to include. The default is 3.
    #[arg(long)]
    limit: Option<u64>,
    /// Pings only. With --all, every ping. Without it, open pings.
    #[arg(long)]
    ping: bool,
}

#[derive(Args)]
struct PrArgs {
    #[arg(long)]
    id: u64,
    #[arg(long)]
    pr: String,
    #[arg(long)]
    parent: Option<u64>,
    #[arg(long)]
    body: Option<String>,
    #[arg(long)]
    mention: Vec<String>,
}

#[derive(Args)]
struct MergeArgs {
    #[arg(long)]
    id: u64,
    #[arg(long)]
    sha: String,
    #[arg(long)]
    at: Option<String>,
    #[arg(long)]
    parent: Option<u64>,
    #[arg(long)]
    body: Option<String>,
    #[arg(long)]
    mention: Vec<String>,
    /// Agent who is merging. When set, an assignee waiting on this plan is woken.
    #[arg(long)]
    agent: Option<String>,
}

#[derive(Args)]
struct WorktreeArgs {
    #[arg(long)]
    id: u64,
    #[arg(long)]
    path: Option<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long)]
    clear: bool,
    #[arg(long)]
    parent: Option<u64>,
    #[arg(long)]
    body: Option<String>,
}

#[derive(Args)]
struct ListArgs {
    /// open, blocked, pinned, closed, cancelled, or repeat the flag. Default is open and blocked.
    #[arg(long)]
    status: Vec<String>,
    /// plan or item.
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    repo: Option<String>,
    /// Include every status.
    #[arg(long)]
    all: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let root = resolve_root(cli.dir.as_deref())?;
    let store = Store::new(root.clone());
    match cli.cmd {
        Command::Init => {
            let data = store.init()?;
            finish(cli.json, serde_json::json!({ "data": data }), || {
                println!("{}", data.display());
            });
        }
        Command::Owners => {
            let owners = comms::owners::load(&root)?;
            finish(cli.json, &owners, || {
                for (repo, agent) in &owners {
                    println!("{repo}  {agent}");
                }
            });
        }
        Command::Plan(PlanCmd::Open(args)) => {
            let event = store.plan_open(
                args.plan_ref,
                args.path,
                args.repo,
                args.depends_on,
                args.summary,
                args.tag,
                args.entity,
            )?;
            emit_event(cli.json, &event);
        }
        Command::Plan(PlanCmd::Update(args)) => {
            if let Some(status) = &args.status {
                check_status(status)?;
            }
            let depends_on = replace_list(args.clear_depends, args.depends_on, "depends")?;
            let tags = replace_list(args.clear_tags, args.tag, "tags")?;
            let entities = replace_list(args.clear_entities, args.entity, "entities")?;
            if args.path.is_none()
                && args.summary.is_none()
                && args.status.is_none()
                && depends_on.is_none()
                && tags.is_none()
                && entities.is_none()
            {
                bail!("plan update needs a field to change");
            }
            let (path, content_hash) = match args.path {
                Some(path) => {
                    let full = canonicalize_existing(&path)?;
                    let hash = hash_file(&full)?;
                    (Some(full.display().to_string()), Some(hash))
                }
                None => (None, None),
            };
            let event = store.plan_update(Op::PlanUpdate {
                plan_ref: args.plan_ref,
                path,
                content_hash,
                depends_on,
                summary: args.summary,
                tags,
                entities,
                status: args.status,
            })?;
            emit_event(cli.json, &event);
        }
        Command::Item(ItemCmd::Open(args)) => {
            let event = store.item_open(Op::ItemOpen {
                plan_ref: args.plan,
                repo: args.repo,
                worktree: args.worktree,
                branch: args.branch,
                requesting_agent: args.requesting,
                requested_agent: args.requested,
                summary: args.summary,
                tags: args.tag,
                entities: args.entity,
            })?;
            emit_event(cli.json, &event);
        }
        Command::Notes(cmd) => run_notes(cli.json, &store, cmd)?,
        Command::Note(args) => emit_event(
            cli.json,
            &store.act(action(
                args.id,
                args.parent,
                "note",
                args.body,
                args.mention,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
            ))?,
        ),
        Command::Block(args) => emit_event(
            cli.json,
            &store.act(action(
                args.id,
                args.parent,
                "block",
                args.body,
                args.mention,
                Some("blocked".into()),
                Some(args.by),
                false,
                args.requested,
                None,
                None,
                None,
                None,
                None,
                false,
            ))?,
        ),
        Command::Unblock(args) => {
            let mut op = action(
                args.id,
                args.parent,
                "unblock",
                args.body.unwrap_or_else(|| "unblocked".into()),
                args.mention,
                Some("open".into()),
                None,
                true,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
            );
            if let Op::Action { agent, .. } = &mut op {
                *agent = args.agent;
            }
            emit_event(cli.json, &store.act(op)?);
        }
        Command::Pin(args) => emit_event(
            cli.json,
            &store.act(action(
                args.id,
                args.parent,
                "pin",
                args.body.unwrap_or_else(|| "pinned".into()),
                args.mention,
                Some("pinned".into()),
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
            ))?,
        ),
        Command::Supersede(args) => {
            let events = store.supersede(args.id, args.agent, args.by, args.body, args.mention)?;
            finish(cli.json, &events, || {
                for event in &events {
                    println!("#{}  {}", event.id, event_label(&event.op));
                }
            });
        }
        Command::Close(args) => {
            let mut op = action(
                args.id,
                args.parent,
                "close",
                args.body.unwrap_or_else(|| "closed".into()),
                args.mention,
                Some("closed".into()),
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
            );
            if let Op::Action { agent, .. } = &mut op {
                *agent = args.agent;
            }
            emit_event(cli.json, &store.act(op)?);
        }
        Command::Handoff(args) => {
            let body = args
                .body
                .clone()
                .unwrap_or_else(|| format!("handoff to {}", args.requested));
            let events = store.handoff(
                args.id,
                args.agent,
                args.requested,
                args.parent,
                body,
                args.mention,
            )?;
            finish(cli.json, &events, || {
                for event in &events {
                    println!("#{}  {}", event.id, event_label(&event.op));
                }
            });
        }
        Command::Tell(args) => {
            let kind = match (args.need, args.fyi) {
                (true, false) => "need",
                (false, _) => "tell",
                (true, true) => bail!("tell takes --need or --fyi"),
            };
            let on = store.resolve_on(&args.on)?;
            let desk = desk_need(&args)?;
            let events = store.tell(
                on,
                args.agent,
                args.to,
                args.all,
                kind.into(),
                args.body,
                args.mention,
                args.thread,
                desk,
            )?;
            finish(cli.json, &events, || {
                for event in &events {
                    println!("#{}  {}", event.id, event_label(&event.op));
                }
            });
        }
        Command::Notice(args) => {
            let on = store.resolve_on(&args.on)?;
            let events = store.notice(
                on,
                args.agent,
                args.to,
                args.body,
                args.mention,
                args.thread,
            )?;
            finish(cli.json, &events, || {
                for event in &events {
                    println!("#{}  {}", event.id, event_label(&event.op));
                }
            });
        }
        Command::Reply(args) => {
            let events = store.reply(args.id, args.agent, args.body, args.thread)?;
            finish(cli.json, &events, || {
                for event in &events {
                    println!("#{}  {}", event.id, event_label(&event.op));
                }
            });
        }
        Command::Answer(args) => {
            let event = store.answer(
                args.id,
                args.agent,
                args.kind,
                args.body,
                args.to,
                args.outcome,
                args.thread,
                args.signature,
                args.signer,
                args.public_key,
            )?;
            finish(cli.json, &event, || {
                println!("#{}  {}", event.id, event_label(&event.op));
            });
        }
        Command::Ack(args) => {
            let (outcome, body) = ack_outcome(args.did, args.later, args.ask_founder, args.ackn, args.answered)?;
            let events = store.ack(args.id, args.agent, outcome, body, args.thread)?;
            finish(cli.json, &events, || {
                for event in &events {
                    println!("#{}  {}", event.id, event_label(&event.op));
                }
            });
        }
        Command::Withdraw(args) => {
            let event = store.withdraw(args.id, args.agent, args.body)?;
            finish(cli.json, &event, || {
                println!("#{}  {}", event.id, event_label(&event.op));
            });
        }
        Command::Thread(args) => {
            let event = store.link_thread(args.id, args.agent, args.thread)?;
            finish(cli.json, &event, || {
                println!("#{}  {}", event.id, event_label(&event.op));
            });
        }
        Command::Messages(args) => {
            let rows = if args.channel {
                store.channel_messages(
                    args.from.as_deref(),
                    args.to.as_deref(),
                    args.record.as_deref(),
                    args.all,
                    args.limit.unwrap_or(3),
                )?
            } else {
                store.messages(
                    args.from.as_deref(),
                    args.to.as_deref(),
                    args.record.as_deref(),
                    args.all,
                )?
            };
            let rows = if args.ping {
                rows.into_iter().filter(|row| row.desk.ping).collect()
            } else {
                rows
            };
            finish(cli.json, &rows, || {
                if rows.is_empty() {
                    println!(
                        "No data returned. The ledger has no messages that match this filter."
                    );
                    return;
                }
                let mut table =
                    new_table(vec!["ID", "WHEN", "FROM", "TO", "RECORD", "STATUS", "BODY"]);
                for row in &rows {
                    let record = match &row.plan_ref {
                        Some(plan) => format!("#{} {}", row.record_id, plan),
                        None => format!("#{}", row.record_id),
                    };
                    table.add_row(vec![
                        row.id.to_string(),
                        short_at(&row.at),
                        row.from.clone().unwrap_or_default(),
                        row.to.clone().unwrap_or_default(),
                        record,
                        row.status.clone(),
                        row.body.clone(),
                    ]);
                }
                println!("{table}");
            });
        }
        Command::Pr(args) => {
            let body = args.body.unwrap_or_else(|| format!("pr {}", args.pr));
            emit_event(
                cli.json,
                &store.act(action(
                    args.id,
                    args.parent,
                    "pr",
                    body,
                    args.mention,
                    None,
                    None,
                    false,
                    None,
                    Some(args.pr),
                    None,
                    None,
                    None,
                    None,
                    false,
                ))?,
            );
        }
        Command::Merge(args) => {
            if let Some(at) = &args.at {
                comms::model::check_time(at)?;
            }
            let body = args.body.unwrap_or_else(|| format!("merged {}", args.sha));
            let at = args.at.unwrap_or_else(comms::model::now_utc);
            let mut op = action(
                    args.id,
                    args.parent,
                    "merge",
                    body,
                    args.mention,
                    Some("closed".into()),
                    None,
                    false,
                    None,
                    None,
                    Some(args.sha),
                    Some(at),
                    None,
                    None,
                    false,
                );
            if let Op::Action { agent, .. } = &mut op {
                *agent = args.agent;
            }
            emit_event(cli.json, &store.act(op)?);
        }
        Command::Worktree(args) => {
            if args.clear && args.path.is_some() {
                bail!("pass either --clear or --path");
            }
            let body = args.body.unwrap_or_else(|| {
                if args.clear {
                    "worktree cleared".into()
                } else {
                    "worktree set".into()
                }
            });
            emit_event(
                cli.json,
                &store.act(action(
                    args.id,
                    args.parent,
                    "worktree",
                    body,
                    vec![],
                    None,
                    None,
                    false,
                    None,
                    None,
                    None,
                    None,
                    args.path,
                    args.branch,
                    args.clear,
                ))?,
            );
        }
        Command::List(args) => {
            if let Some(kind) = &args.kind {
                if kind != "plan" && kind != "item" {
                    bail!("kind must be plan or item");
                }
            }
            let statuses = if args.all {
                Vec::new()
            } else if args.status.is_empty() {
                vec!["open".into(), "blocked".into()]
            } else {
                for status in &args.status {
                    check_status(status)?;
                }
                args.status
            };
            let rows = store.list(args.kind.as_deref(), args.repo.as_deref(), &statuses)?;
            finish(cli.json, &rows, || print_records(&rows));
        }
        Command::Ready => {
            let rows = store.ready()?;
            finish(cli.json, &rows, || print_ready(&rows));
        }
        Command::Show { key } => {
            let show = store.show(&key)?;
            finish(cli.json, &show, || print_show(&show));
        }
        Command::Related { key } => {
            let rows = store.related(&key)?;
            finish(cli.json, &rows, || print_records(&rows));
        }
        Command::Search { words } => {
            let rows = store.search(&words.join(" "))?;
            finish(cli.json, &rows, || {
                let mut table = new_table(vec!["ID", "Record", "Plan", "Kind", "Owner", "Snippet"]);
                for hit in &rows {
                    table.add_row(vec![
                        hit.id.to_string(),
                        hit.record_id.to_string(),
                        hit.plan_ref.clone().unwrap_or_default(),
                        hit.kind.clone(),
                        hit.owner.clone(),
                        hit.snippet.clone(),
                    ]);
                }
                println!("{table}");
            });
        }
        Command::Tree { id } => {
            let show = store.show(&id.to_string())?;
            finish(cli.json, &show.actions, || {
                if show.actions.is_empty() {
                    println!("no actions");
                    return;
                }
                print_tree(&show.actions, None, 0);
            });
        }
        Command::Listen(args) => {
            comms::listen::run(
                &store,
                args.agent.trim(),
                std::time::Duration::from_secs(args.refresh),
                args.orch,
            )?;
        }
        Command::Rebuild => {
            store.rebuild()?;
            finish(cli.json, serde_json::json!({ "rebuilt": true }), || {
                println!("rebuilt");
            });
        }
    }
    Ok(())
}

fn run_notes(json_mode: bool, store: &Store, cmd: NotesCmd) -> Result<()> {
    let notes = comms::notes::Notes::open(comms::notes::Notes::default_path()?);
    match cmd {
        NotesCmd::New(args) => {
            let note = notes.create(&args.author, &args.title, &args.body, args.tag, args.source)?;
            if json_mode {
                println!("{}", serde_json::to_string(&note)?);
            } else {
                println!("#{} {}", note.id, note.title);
            }
        }
        NotesCmd::List => {
            let rows = notes.list()?;
            if json_mode {
                println!("{}", serde_json::to_string(&rows)?);
            } else if rows.is_empty() {
                println!("No notes.");
            } else {
                for note in rows {
                    println!("#{} {} {}", note.id, note.status, note.title);
                }
            }
        }
        NotesCmd::Show(args) => {
            let note = notes.show(args.id)?;
            println!("{}", serde_json::to_string_pretty(&note)?);
        }
        NotesCmd::Drop(args) => {
            let note = notes.drop(args.id)?;
            println!("#{} dropped", note.id);
        }
        NotesCmd::Promote(args) => {
            check_agent("agent", &args.agent)?;
            let line = notes.summary_for_ledger(args.id, &args.summary)?;
            let on = store.resolve_on(&args.on)?;
            let event = store.act(action(
                on,
                None,
                "note",
                line,
                vec![],
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
            ))?;
            notes.mark_promoted(args.id, &format!("#{on}"))?;
            emit_event(json_mode, &event);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn action(
    record_id: u64,
    parent_id: Option<u64>,
    kind: &str,
    body: String,
    mentions: Vec<String>,
    status: Option<String>,
    blocked_by: Option<String>,
    clear_blocked_by: bool,
    requested_agent: Option<String>,
    pr_id: Option<String>,
    merge_sha: Option<String>,
    merge_at: Option<String>,
    worktree: Option<String>,
    branch: Option<String>,
    clear_worktree: bool,
) -> Op {
    Op::Action {
        record_id,
        parent_id,
        kind: kind.into(),
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
        agent: None,
        pivot: None,
        to: None,
        outcome: None,
        thread: vec![],
    
        desk: DeskNeed::default(),
}
}

fn desk_need(args: &TellArgs) -> Result<DeskNeed> {
    let mut option = Vec::new();
    for line in &args.option_line {
        let mut parts = line.split('|');
        let label = parts.next().unwrap_or("").trim().to_string();
        let allows = parts.next().unwrap_or("").trim().to_string();
        let cost = parts.next().map(str::trim).filter(|part| !part.is_empty()).map(str::to_string);
        let risk = parts.next().map(str::trim).filter(|part| !part.is_empty()).map(str::to_string);
        option.push(DeskOption { label, allows, cost, risk });
    }
    let mut data = Vec::new();
    for line in &args.data_line {
        let Some((label, number)) = line.split_once('|') else {
            bail!("data is label|number");
        };
        let number: f64 = number.trim().parse().context("data number")?;
        data.push(DeskDatum { label: label.trim().to_string(), number });
    }
    Ok(DeskNeed {
        urgency: args.urgency,
        category: args.category.clone(),
        profile: args.profile.clone(),
        title: args.title.clone(),
        option,
        human_step: args.human_step.clone(),
        action: args.action.clone(),
        why: args.why.clone(),
        standard: args.standard.clone(),
        link: args.link.clone(),
        ping: args.ping,
        for_founder: args.for_founder,
        via: args.via.clone(),
        data,
        signature: None,
        signer: None,
        queue_kind: args.queue_kind.clone(),
    })
}

fn ack_outcome(
    did: Option<String>,
    later: Option<String>,
    ask_founder: Option<String>,
    ackn: Option<String>,
    answered: Option<String>,
) -> Result<(String, String)> {
    let chosen = [
        did.as_ref(),
        later.as_ref(),
        ask_founder.as_ref(),
        ackn.as_ref(),
        answered.as_ref(),
    ]
    .into_iter()
    .filter(|value| value.is_some())
    .count();
    if chosen != 1 {
        bail!("ack takes one of --did, --later, --ask-founder, --ackn, or --answered");
    }
    if let Some(body) = did {
        return Ok(("did".into(), body));
    }
    if let Some(body) = later {
        return Ok(("later".into(), body));
    }
    if let Some(body) = ackn {
        return Ok(("ackn".into(), body));
    }
    if let Some(body) = answered {
        return Ok(("answered".into(), body));
    }
    Ok(("ask-founder".into(), ask_founder.unwrap_or_default()))
}

fn replace_list(clear: bool, values: Vec<String>, label: &str) -> Result<Option<Vec<String>>> {
    if clear && !values.is_empty() {
        bail!("--clear-{label} replaces the list; omit the extra values");
    }
    if clear {
        Ok(Some(Vec::new()))
    } else if values.is_empty() {
        Ok(None)
    } else {
        Ok(Some(values))
    }
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

fn emit_event(json_mode: bool, event: &comms::model::Event) {
    finish(json_mode, event, || {
        println!("#{}  {}", event.id, event_label(&event.op));
    });
}

fn short_at(at: &str) -> String {
    if at.len() >= 17 {
        return format!("{} {}Z", &at[5..10], &at[11..16]);
    }
    at.to_string()
}

fn event_label(op: &Op) -> String {
    match op {
        Op::PlanOpen { plan_ref, .. } => format!("plan {plan_ref}"),
        Op::PlanUpdate { plan_ref, .. } => format!("plan update {plan_ref}"),
        Op::ItemOpen { plan_ref, .. } => format!("item {plan_ref}"),
        Op::Action {
            kind, record_id, ..
        } => format!("{kind} on #{record_id}"),
        Op::Thread { message_id, .. } => format!("thread on #{message_id}"),
    }
}

fn finish(json_mode: bool, value: impl serde::Serialize, text: impl FnOnce()) {
    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
        );
    } else {
        text();
    }
    let _ = std::io::stdout().flush();
}

fn print_records(rows: &[OwnedRecord]) {
    if rows.is_empty() {
        println!("no records");
        return;
    }
    let mut table = new_table(vec![
        "ID",
        "Kind",
        "Ref",
        "Status",
        "Owner",
        "Requested",
        "Summary",
    ]);
    for row in rows {
        table.add_row(vec![
            row.record.id.to_string(),
            row.record.kind.clone(),
            row.record.plan_ref.clone().unwrap_or_default(),
            paint(&row.record.status),
            row.owner.clone(),
            row.record.requested_agent.clone().unwrap_or_default(),
            row.record.summary.clone(),
        ]);
    }
    println!("{table}");
}

fn print_ready(rows: &[ReadyPlan]) {
    println!("Open plans whose dependencies are closed. Id order is only for display.");
    if rows.is_empty() {
        println!("none");
        return;
    }
    let mut table = new_table(vec!["ID", "Plan", "Owner", "Depends", "Summary"]);
    for row in rows {
        table.add_row(vec![
            row.record.id.to_string(),
            row.record.plan_ref.clone().unwrap_or_default(),
            row.owner.clone(),
            if row.record.depends_on.is_empty() {
                "—".into()
            } else {
                row.record.depends_on.join(", ")
            },
            row.record.summary.clone(),
        ]);
        for item in &row.items {
            table.add_row(vec![
                item.record.id.to_string(),
                format!(
                    "  item {}",
                    item.record.plan_ref.clone().unwrap_or_default()
                ),
                item.owner.clone(),
                item.record.requested_agent.clone().unwrap_or_default(),
                item.record.summary.clone(),
            ]);
        }
    }
    println!("{table}");
}

fn print_show(show: &Show) {
    println!(
        "#{}  {}  {}  {}  owner {}",
        show.record.id,
        show.record.kind,
        show.record.plan_ref.clone().unwrap_or_default(),
        paint(&show.record.status),
        show.owner
    );
    println!("repo: {}", show.record.repo);
    if let Some(path) = &show.record.path {
        println!("path: {path}");
    }
    if let Some(hash) = &show.record.content_hash {
        println!("hash: {hash}");
    }
    if !show.record.depends_on.is_empty() {
        println!("depends on: {}", show.record.depends_on.join(", "));
    }
    if let Some(by) = &show.record.blocked_by {
        println!("blocked by: {by}");
    }
    if let Some(agent) = &show.record.requesting_agent {
        println!("requesting: {agent}");
    }
    if let Some(agent) = &show.record.requested_agent {
        println!("requested: {agent}");
    }
    if let Some(worktree) = &show.record.worktree {
        println!("worktree: {worktree}");
    }
    if let Some(branch) = &show.record.branch {
        println!("branch: {branch}");
    }
    if let Some(pr) = &show.record.pr_id {
        println!("pr: {pr}");
    }
    if let Some(sha) = &show.record.merge_sha {
        println!(
            "merge: {sha} {}",
            show.record.merge_at.clone().unwrap_or_default()
        );
    }
    if !show.record.tags.is_empty() {
        println!("tags: {}", show.record.tags.join(", "));
    }
    if !show.record.entities.is_empty() {
        println!("entities: {}", show.record.entities.join(", "));
    }
    println!("summary: {}", show.record.summary);
    if !show.items.is_empty() {
        println!("\nitems");
        print_records(&show.items);
    }
    if show.actions.is_empty() {
        return;
    }
    println!("\nactions");
    print_tree(&show.actions, None, 0);
}

fn print_tree(actions: &[Action], parent: Option<u64>, depth: usize) {
    for action in actions.iter().filter(|action| action.parent_id == parent) {
        let indent = "  ".repeat(depth);
        let marker = if depth == 0 { "" } else { "└ " };
        println!(
            "{indent}{marker}#{} {}  {}",
            action.id, action.kind, action.body
        );
        if action.agent.is_some() || action.pivot.is_some() {
            println!(
                "{indent}  agent {}  pivot {}",
                action.agent.clone().unwrap_or_else(|| "—".into()),
                action.pivot.clone().unwrap_or_else(|| "—".into())
            );
        }
        print_tree(actions, Some(action.id), depth + 1);
    }
}

fn new_table(header: Vec<&str>) -> Table {
    let mut table = Table::new();
    if std::io::stdout().is_terminal() {
        table.load_preset(presets::UTF8_FULL);
    } else {
        table.load_preset(presets::ASCII_NO_BORDERS);
    }
    table.set_header(header);
    table
}

fn paint(status: &str) -> String {
    if !std::io::stdout().is_terminal() {
        return status.to_string();
    }
    let color = match status {
        "open" => "32",
        "blocked" => "31",
        "pinned" => "33",
        "closed" | "cancelled" => "90",
        _ => "0",
    };
    format!("\x1b[{color}m{status}\x1b[0m")
}
