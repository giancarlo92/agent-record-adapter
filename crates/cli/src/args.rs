// Part of Agent Record Adapter (see NOTICE).
//! The commands `agent-record` runs itself. Every other command is
//! agent-desktop's and goes to the engine unchanged (`External`).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "agent-record",
    version,
    about = "Desktop apps (agent-desktop), web pages (agent-browser) and recording, in the background",
    allow_external_subcommands = true,
    disable_help_subcommand = true
)]
pub(crate) struct Cli {
    /// Session to run in (as agent-desktop's --session).
    #[arg(long, global = true)]
    pub session: Option<String>,
    /// Agent identity for the drawn cursor (as agent-desktop's --agent-id).
    #[arg(long = "agent-id", global = true)]
    pub agent_id: Option<String>,
    /// Allow input through the person's real pointer and keyboard.
    #[arg(long, global = true)]
    pub headed: bool,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

// Parsed once per run: the size of the largest variant does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    #[command(
        about = "Record the user's own clicks, typing, key presses and scrolls (and a video) to a JSON Lines file until stopped; record start/stop run it in the background"
    )]
    Record(RecordArgs),
    #[command(
        about = "Store passwords once and write them into fields by name, never showing them (secret set/list/delete/fill)"
    )]
    Secret(SecretArgs),
    #[command(
        about = "Drag with the left button along a path of points, such as a line drawn on a canvas (requires --headed)"
    )]
    Stroke(StrokeArgs),
    #[command(
        about = "Run a command of a window's menu bar by its path (File, Save as...), without opening the menu or taking the focus"
    )]
    Menu(MenuArgs),
    #[command(
        about = "Click a point of a window, in pixels of its screenshot, for content not in the accessibility tree; never moves the cursor or takes the focus"
    )]
    ClickAt(ClickAtArgs),
    #[command(
        about = "Open an address in the tab a browser window without DevTools shows (the person's own browser), through its address bar, never taking the focus; pages with DevTools: use `browser`"
    )]
    Navigate(NavigateArgs),
    #[command(
        about = "Drive web pages through agent-browser, in the background: `agent-record browser open URL`, `snapshot`, `click @e1`... (every agent-browser command)"
    )]
    Browser(BrowserArgs),
    #[command(
        name = "list-displays",
        about = "List the displays with their number (1, 2, ...), id, bounds and scale; record --display takes the number"
    )]
    ListDisplays,
    /// Any other command: the engine gets the whole command line as it was.
    #[command(external_subcommand)]
    External(#[allow(dead_code)] Vec<String>),
}

#[derive(Args, Debug)]
pub(crate) struct WindowScope {
    #[arg(long, help = "The application whose window to act on")]
    pub app: Option<String>,
    #[arg(
        long = "window-id",
        help = "The window to act on (an id from list-windows)"
    )]
    pub window_id: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct MenuArgs {
    #[command(flatten)]
    pub scope: WindowScope,
    #[arg(
        value_name = "ITEM",
        required_unless_present_any = ["shortcut", "find"],
        help = "The menu path, one item per argument, such as: File Export (shortcut letters, keys and a trailing ... are ignored)"
    )]
    pub path: Vec<String>,
    #[arg(
        long,
        value_name = "KEYS",
        conflicts_with_all = ["path", "find"],
        help = "Run the item that shows these keys, anywhere in the menu bar, such as ctrl+enter"
    )]
    pub shortcut: Option<String>,
    #[arg(long, value_name = "NAME", conflicts_with_all = ["path"], help = "Run the item with this name, in any submenu")]
    pub find: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ClickAtArgs {
    #[command(flatten)]
    pub scope: WindowScope,
    #[arg(
        allow_negative_numbers = true,
        help = "Pixels from the left edge of the window's screenshot"
    )]
    pub x: i32,
    #[arg(
        allow_negative_numbers = true,
        help = "Pixels from the top edge of the window's screenshot"
    )]
    pub y: i32,
    #[arg(long, help = "Double-click instead of a single click")]
    pub double: bool,
    #[arg(
        long,
        help = "x and y are screen pixels (as element bounds are), not pixels of the screenshot"
    )]
    pub screen: bool,
}

#[derive(Args, Debug)]
pub(crate) struct NavigateArgs {
    #[command(flatten)]
    pub scope: WindowScope,
    #[arg(help = "The address to open (http, https or file)")]
    pub url: String,
    #[arg(
        long = "in-app",
        help = "Change the address the way a single-page app's own links do"
    )]
    pub in_app: bool,
}

#[derive(Args, Debug)]
pub(crate) struct BrowserArgs {
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help = "agent-browser's command and arguments"
    )]
    pub args: Vec<String>,
}

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub(crate) struct RecordArgs {
    #[command(subcommand)]
    pub action: Option<RecordAction>,
    #[arg(
        long,
        help = "JSON Lines file the recording is streamed to (required without start/stop)"
    )]
    pub out: Option<PathBuf>,
    #[command(flatten)]
    pub scope: RecordScope,
    #[arg(long, help = "Stop when this file appears; it must not exist yet")]
    pub stop_file: Option<PathBuf>,
    #[command(flatten)]
    pub video: RecordVideo,
    #[arg(long, help = "Do not show the on-screen recording indicator")]
    pub no_indicator: bool,
}

#[derive(Subcommand, Debug)]
pub(crate) enum RecordAction {
    #[command(
        about = "Start recording in the background and return at once with the session's id and paths"
    )]
    Start(RecordStartArgs),
    #[command(about = "Stop a background recording and return its summary")]
    Stop(RecordStopArgs),
    #[command(
        about = "Show the background recordings and the displays there are to record (with their numbers)"
    )]
    Status,
}

#[derive(Args, Debug)]
pub(crate) struct RecordStartArgs {
    #[arg(
        long,
        help = "JSON Lines file the recording is streamed to (default: <state root>/recordings/<id>.jsonl)"
    )]
    pub out: Option<PathBuf>,
    #[command(flatten)]
    pub scope: RecordScope,
    #[command(flatten)]
    pub video: RecordVideo,
    #[arg(long, help = "Do not show the on-screen recording indicator")]
    pub no_indicator: bool,
}

#[derive(Args, Debug)]
pub(crate) struct RecordStopArgs {
    #[arg(
        long,
        help = "The recording to stop (needed when more than one is running)"
    )]
    pub id: Option<String>,
    #[arg(
        long,
        help = "How long to wait for the session to finish, video included (default 60000)"
    )]
    pub timeout_ms: Option<u64>,
}

#[derive(Args, Debug)]
pub(crate) struct RecordScope {
    #[arg(long, help = "Keep only actions in this application's windows")]
    pub app: Option<String>,
    #[arg(
        long = "window-id",
        help = "Keep only actions in this window (an id from list-windows)"
    )]
    pub window_id: Option<String>,
    #[arg(
        long,
        help = "Keep only actions on this display: primary, its number in list-displays (1, 2, ...) or its id"
    )]
    pub display: Option<String>,
    #[arg(
        long,
        help = "Stop after this many milliseconds (default 600000, at most 3600000)"
    )]
    pub max_duration_ms: Option<u64>,
    #[arg(
        long,
        help = "Also record input Windows marks as injected (synthesized by software)"
    )]
    pub include_injected: bool,
}

#[derive(Args, Debug)]
pub(crate) struct RecordVideo {
    #[arg(
        long,
        help = "Also record an MP4 video of the recorded window or display"
    )]
    pub video: bool,
    #[arg(long, help = "Write the video to this .mp4 file (implies --video)")]
    pub video_out: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct SecretArgs {
    #[command(subcommand)]
    pub action: SecretAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum SecretAction {
    #[command(
        about = "Save a secret under a name; it is typed at a hidden prompt, never passed as an argument"
    )]
    Set(SecretSetArgs),
    #[command(
        about = "List the saved secrets' names and what each may be written into (never their values)"
    )]
    List,
    #[command(about = "Delete a saved secret")]
    Delete(SecretNameArgs),
    #[command(
        about = "Write a saved secret into a field, by name, after checking the field, app and site it was saved for"
    )]
    Fill(SecretFillArgs),
}

#[derive(Args, Debug)]
pub(crate) struct SecretSetArgs {
    #[arg(help = "The secret's name: letters, digits, '-', '_' or '.' (e.g. bank-login)")]
    pub name: String,
    #[arg(long, help = "Write it only into this app's windows (e.g. chrome.exe)")]
    pub app: Option<String>,
    #[arg(
        long,
        help = "Write it only on this site or its subdomains (e.g. login.example.com)"
    )]
    pub site: Option<String>,
    #[arg(long, help = "Also allow fields that are not password fields")]
    pub any_field: bool,
    #[arg(
        long,
        help = "Read the value from the first line of standard input instead of a prompt"
    )]
    pub stdin: bool,
}

#[derive(Args, Debug)]
pub(crate) struct SecretNameArgs {
    #[arg(help = "The secret's name")]
    pub name: String,
}

#[derive(Args, Debug)]
pub(crate) struct SecretFillArgs {
    #[arg(
        value_name = "REF",
        help = "The field to write into (@<snapshot_id>:eN)"
    )]
    pub ref_id: String,
    #[arg(help = "The secret's name")]
    pub name: String,
    #[arg(
        long = "timeout-ms",
        help = "Maximum wait for the field, in milliseconds"
    )]
    pub timeout_ms: Option<u64>,
}

#[derive(Args, Debug)]
pub(crate) struct StrokeArgs {
    #[arg(
        long,
        help = "Points as x,y;x,y;... or a JSON array of [x, y] or [x, y, ms] (a recorded drag's path); requires --headed"
    )]
    pub path: Option<String>,
    #[arg(long, help = "Read the path from this file instead (same formats)")]
    pub path_file: Option<PathBuf>,
    #[arg(
        long,
        help = "Milliseconds between points that carry no time of their own [default: 16]"
    )]
    pub step_ms: Option<u64>,
    #[arg(
        long,
        help = "Add this to every x and y (a recorded path replayed in a window that moved); write a negative value with = (--offset=-10,5)"
    )]
    pub offset: Option<String>,
}
