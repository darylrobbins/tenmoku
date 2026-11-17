//! Show Memory Usage (`memory.show`): what each part of the app says it
//! holds, beside the process's totals and the remainder no part claims, in
//! a read-only tab; `tenmoku --perf` prints the same table.
//!
//! - Each crate states its own sizes through plain methods (no shared trait,
//!   no counting allocator), and [`MemoryReport::collect`] knows their
//!   owners: the panes' editors and viewers, the project panel, quick open,
//!   the symbol index, the terminals and the language servers. Everything
//!   is read when asked, never kept up to date.
//! - A buffer that several editors show is counted once, by its id; each
//!   editor's own syntax trees are counted for it.
//! - The process's totals come from the OS ([`ProcessMemory::current`]):
//!   on macOS its physical footprint, the graphics part of it and malloc's
//!   bytes in use; on Linux the resident set and glibc's; on Windows the
//!   working set. What a platform can't say is "not measured".
//! - Unattributed is malloc's bytes in use less the rows' bytes: negative
//!   where estimates overshoot.
//! - Language servers are processes of their own, listed after the table
//!   with their children and memory read by pid, not in its totals, and
//!   rust-analyzer's own breakdown (`rust-analyzer/memoryUsage`) under its
//!   row. A Restart lens over each runs `lsp.restart` for it.

use std::{collections::HashMap, path::PathBuf, pin::pin, sync::Arc, time::Duration};

use fe_buffer::{Buffer, BufferId};
use fe_command::{Invocation, Value};
use fe_editor::EditorModel;
use fe_gpui::{EditorMemory, Lens, LensItem, LensProvider};
use fe_lsp::{LspError, ServerId};
use futures::future::{self, Either};
use gpui::{App, Context, Focusable as _, SharedString, Window};
use kiln_l10n::tr;

use crate::{
    Workbench,
    workbench::toast::{self, Toast},
};

/// The rows' stable ids, in the order the table lists them.
pub mod module {
    pub const FOOTPRINT: &str = "process.footprint";
    pub const GRAPHICS: &str = "process.graphics";
    pub const RESIDENT: &str = "process.resident";
    pub const MALLOC: &str = "process.malloc";
    pub const BUFFER_TEXT: &str = "buffer.text";
    pub const UNDO: &str = "buffer.undo";
    pub const SYNTAX_TREES: &str = "syntax.trees";
    pub const WASM_STORES: &str = "syntax.wasm_stores";
    pub const WASM_GRAMMARS: &str = "syntax.wasm_grammars";
    pub const SYMBOL_INDEX: &str = "nav.symbol_index";
    pub const PROJECT_TREE: &str = "project.tree";
    pub const FINDER: &str = "project.finder";
    pub const FILE_VIEWS: &str = "fileview.cache";
    pub const TERMINALS: &str = "terminal.scrollback";
    pub const LSP: &str = "lsp.client";
    pub const MINIMAP: &str = "editor.minimap";
    pub const UNATTRIBUTED: &str = "unattributed";

    /// The process's totals, which the table lists first.
    pub const PROCESS: [&str; 4] = [FOOTPRINT, GRAPHICS, RESIDENT, MALLOC];
}

/// The rust-analyzer request for its own memory breakdown.
const MEMORY_USAGE: &str = "rust-analyzer/memoryUsage";
/// How long the report waits for rust-analyzer's breakdown.
const MEMORY_USAGE_TIMEOUT: Duration = Duration::from_secs(10);
const MB: f64 = 1024. * 1024.;

/// One line of the table.
#[derive(Clone, Debug, PartialEq)]
pub struct MemoryRow {
    /// Stable: see [`module`].
    pub module: &'static str,
    pub label: SharedString,
    /// `None` where it isn't measured. Only unattributed is ever negative.
    pub bytes: Option<i64>,
    /// What it counts (buffers, nodes, terminals …), where it counts any.
    pub count: Option<u64>,
    /// Whether `bytes` is an estimate.
    pub estimate: bool,
}

impl MemoryRow {
    fn new(module: &'static str, label: SharedString, bytes: Option<u64>) -> Self {
        Self {
            module,
            label,
            bytes: bytes.map(|bytes| bytes as i64),
            count: None,
            estimate: false,
        }
    }

    fn count(mut self, count: impl TryInto<u64>) -> Self {
        self.count = count.try_into().ok();
        self
    }

    fn estimate(mut self) -> Self {
        self.estimate = true;
        self
    }
}

/// The process's totals, as the OS gives them: `None` where it doesn't.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessMemory {
    /// macOS's physical footprint (`phys_footprint`), what Activity
    /// Monitor shows.
    pub footprint: Option<u64>,
    /// The graphics part of the footprint (`ledger_tag_graphics_footprint`):
    /// IOSurfaces and GPU memory.
    pub graphics: Option<u64>,
    /// The resident set (Linux `VmRSS`, the Windows working set).
    pub resident: Option<u64>,
    /// Bytes malloc has handed out and not had back.
    pub malloc_in_use: Option<u64>,
}

impl ProcessMemory {
    pub fn current() -> Self {
        os::process()
    }
}

/// A language server's process or one of its children, read by pid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessUsage {
    pub pid: u32,
    /// The executable's name, if the OS gives it.
    pub name: Option<String>,
    /// How far below the server it is: 0 for the server.
    pub depth: usize,
    pub resident: Option<u64>,
    /// macOS's physical footprint; on Linux, its anonymous memory resident
    /// or swapped out (`RssAnon` + `VmSwap`).
    pub footprint: Option<u64>,
}

/// A running language server, as the report lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerMemory {
    pub id: ServerId,
    /// [`fe_lsp::RunningServer::instance`].
    pub instance: u64,
    /// The workbench's language server manager it runs under: a new root
    /// gets a new manager, whose instances are numbered from 0 again.
    pub generation: u64,
    pub name: String,
    pub root: PathBuf,
    pub pid: Option<u32>,
    /// The documents synced to it.
    pub documents: usize,
    /// The diagnostics it sent for files no editor has open.
    pub diagnostics: usize,
    /// Its process first, then its descendants: none without a pid.
    pub processes: Vec<ProcessUsage>,
    /// rust-analyzer's answer to `rust-analyzer/memoryUsage`, or why there
    /// is none: `None` for any other server.
    pub memory_usage: Option<Result<String, String>>,
}

/// A snapshot of what the app holds: see the module docs.
#[derive(Clone, Debug, PartialEq)]
pub struct MemoryReport {
    pub process: ProcessMemory,
    /// The process's totals first, then each module, then unattributed.
    pub rows: Vec<MemoryRow>,
    pub servers: Vec<ServerMemory>,
}

impl MemoryReport {
    /// Reads `workbench`'s parts and the process's totals now.
    pub fn collect(workbench: &Workbench, cx: &App) -> Self {
        let process = ProcessMemory::current();
        let modules = modules(workbench, cx);
        let servers = servers(workbench);
        let mut rows = vec![
            MemoryRow::new(
                module::FOOTPRINT,
                tr!("workbench-memory-footprint"),
                process.footprint,
            ),
            MemoryRow::new(
                module::GRAPHICS,
                tr!("workbench-memory-graphics"),
                process.graphics,
            ),
            MemoryRow::new(
                module::RESIDENT,
                tr!("workbench-memory-resident"),
                process.resident,
            ),
            MemoryRow::new(
                module::MALLOC,
                tr!("workbench-memory-malloc"),
                process.malloc_in_use,
            ),
        ];
        let attributed: i64 = modules.iter().filter_map(|row| row.bytes).sum();
        let unattributed = process
            .malloc_in_use
            .map(|malloc| malloc as i64 - attributed);
        rows.extend(modules);
        rows.push(MemoryRow {
            module: module::UNATTRIBUTED,
            label: tr!("workbench-memory-unattributed"),
            bytes: unattributed,
            count: None,
            estimate: true,
        });
        Self {
            process,
            rows,
            servers,
        }
    }

    /// [`Self::collect`] now, then each rust-analyzer's own breakdown
    /// (`rust-analyzer/memoryUsage`) under its server, or why there's none:
    /// its error, or no answer within [`MEMORY_USAGE_TIMEOUT`]. What Show
    /// Memory Usage's tab and `tenmoku --perf` both show.
    pub fn collect_with_breakdowns(
        workbench: &Workbench,
        cx: &App,
    ) -> impl Future<Output = Self> + 'static + use<> {
        let mut report = Self::collect(workbench, cx);
        let manager = workbench.lsp.manager();
        let asks: Vec<_> = (report.servers.iter().enumerate())
            .filter(|(_, server)| server.name == "rust-analyzer")
            .filter_map(|(ix, server)| {
                let manager = manager.as_ref()?;
                let manager = manager
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                // Boxed, so the future doesn't borrow the guard.
                let ask: future::BoxFuture<'static, _> =
                    Box::pin(manager.request::<MemoryUsage>(server.id, ()));
                Some((ix, ask))
            })
            .collect();
        let executor = cx.background_executor().clone();
        async move {
            let answers = asks.into_iter().map(|(ix, ask)| {
                let timer = executor.timer(MEMORY_USAGE_TIMEOUT);
                async move {
                    let answer = match future::select(pin!(ask), timer).await {
                        Either::Left((Ok(reply), _)) => Ok(reply.result),
                        Either::Left((Err(error), _)) => Err(lsp_error(&error)),
                        Either::Right(_) => Err(lsp_error(&LspError::Timeout)),
                    };
                    (ix, answer)
                }
            });
            for (ix, answer) in future::join_all(answers).await {
                report.servers[ix].memory_usage = Some(answer);
            }
            report
        }
    }

    /// The row for `module`, if the report has one.
    pub fn row(&self, module: &str) -> Option<&MemoryRow> {
        self.rows.iter().find(|row| row.module == module)
    }

    /// The table, then the language servers: what the tab shows and
    /// `tenmoku --perf` prints.
    pub fn to_text(&self) -> String {
        self.render().0
    }

    /// [`Self::to_text`], and the row each server's line is on.
    fn render(&self) -> (String, Vec<(u32, usize)>) {
        let mut lines = vec![tr!("workbench-memory-title").to_string(), String::new()];
        let width = (self.rows.iter())
            .map(|row| row.label.chars().count())
            .chain([tr!("workbench-memory-column-module").chars().count()])
            .max()
            .unwrap_or_default();
        let not_measured = tr!("workbench-memory-not-measured");
        lines.push(columns(
            &tr!("workbench-memory-column-module"),
            width,
            &tr!("workbench-memory-column-mb"),
            &tr!("workbench-memory-column-count"),
        ));
        for row in &self.rows {
            let bytes = match row.bytes {
                Some(bytes) => {
                    let mark = if row.estimate { "~" } else { "" };
                    format!("{mark}{:.1}", bytes as f64 / MB)
                }
                None => not_measured.to_string(),
            };
            let count = row.count.map(|count| count.to_string()).unwrap_or_default();
            lines.push(columns(&row.label, width, &bytes, &count));
        }
        lines.push(String::new());
        lines.push(tr!("workbench-memory-estimate-note").to_string());
        lines.push(tr!("workbench-memory-unattributed-note").to_string());
        let mut server_rows = Vec::new();
        if !self.servers.is_empty() {
            lines.push(String::new());
            lines.push(tr!("workbench-memory-servers").to_string());
        }
        for (ix, server) in self.servers.iter().enumerate() {
            lines.push(String::new());
            server_rows.push((lines.len() as u32, ix));
            lines.push(server_line(server).to_string());
            let children = server.processes.iter().filter(|process| process.depth > 0);
            lines.extend(children.map(child_line));
            match &server.memory_usage {
                Some(Ok(text)) => {
                    lines.push(format!("  {}:", MEMORY_USAGE));
                    lines.extend(text.lines().map(|line| format!("    {}", line)));
                }
                Some(Err(why)) => {
                    lines.push(format!("  {}: {}", MEMORY_USAGE, why));
                }
                None => {}
            }
        }
        let mut text = lines.join("\n");
        text.push('\n');
        (text, server_rows)
    }
}

/// A table row: the label padded to `width`, then the two columns.
fn columns(label: &str, width: usize, bytes: &str, count: &str) -> String {
    let line = format!("{0:<1$}  {2:>12}  {3:>10}", label, width, bytes, count);
    line.trim_end().to_owned()
}

fn mb(bytes: Option<u64>) -> String {
    match bytes {
        Some(bytes) => format!("{:.1}", bytes as f64 / MB),
        None => tr!("workbench-memory-not-measured").to_string(),
    }
}

fn server_line(server: &ServerMemory) -> SharedString {
    let own = server.processes.first();
    tr!(
        "workbench-memory-server",
        name = server.name.as_str(),
        pid = server.pid.map(|pid| pid.to_string()).unwrap_or_default(),
        resident = mb(own.and_then(|process| process.resident)),
        footprint = mb(own.and_then(|process| process.footprint)),
        documents = server.documents.to_string(),
        diagnostics = server.diagnostics.to_string(),
    )
}

/// A child process's line, indented by its depth (the server's own
/// process is on the server's line).
fn child_line(process: &ProcessUsage) -> String {
    let indent = "  ".repeat(process.depth);
    let line = tr!(
        "workbench-memory-child",
        name = process.name.clone().unwrap_or_default(),
        pid = process.pid.to_string(),
        resident = mb(process.resident),
        footprint = mb(process.footprint),
    );
    format!("{indent}{line}")
}

/// The modules' rows, in [`module`]'s order.
fn modules(workbench: &Workbench, cx: &App) -> Vec<MemoryRow> {
    let mut buffers = HashMap::<BufferId, EditorMemory>::new();
    let (mut tree_nodes, mut tree_bytes) = (0, 0);
    let mut views = HashMap::<*const fe_fileview::FileView, u64>::new();
    for pane in workbench.panes(cx) {
        for tab in pane.read(cx).tabs() {
            if let Some(editor) = tab.editor() {
                let memory = editor.read(cx).memory();
                tree_nodes += memory.tree_nodes;
                tree_bytes += memory.tree_bytes;
                buffers.insert(memory.buffer, memory);
            }
            if let Some(viewer) = tab.viewer() {
                let view = viewer.read(cx).view();
                views.insert(Arc::as_ptr(view), view.cached_bytes());
            }
        }
    }
    let text: u64 = buffers.values().map(|memory| memory.text_bytes).sum();
    let undo: u64 = buffers.values().map(|memory| memory.history_bytes).sum();
    let wasm = fe_syntax::wasm_stats();
    let symbols = workbench.nav_state.index_stats();
    let tree = workbench.panel().read(cx).tree();
    let finder = (workbench.quick_open().read(cx).delegate).index_memory();
    // Not measured if any terminal's isn't: one on the alternate screen.
    let (mut terminals, mut scrollback) = (0, Some(0));
    for view in workbench.terminal().read(cx).views() {
        if let Some(terminal) = view.read(cx).terminal() {
            terminals += 1;
            let bytes = terminal.grid_memory().bytes;
            scrollback = scrollback.zip(bytes).map(|(sum, bytes)| sum + bytes);
        }
    }
    let documents: usize = workbench.lsp.manager().map_or(0, |manager| {
        let manager = manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (manager.running_servers().iter())
            .map(|server| server.documents)
            .sum()
    });
    vec![
        MemoryRow::new(
            module::BUFFER_TEXT,
            tr!("workbench-memory-buffer-text"),
            Some(text),
        )
        .count(buffers.len())
        .estimate(),
        MemoryRow::new(module::UNDO, tr!("workbench-memory-undo"), Some(undo))
            .count(buffers.len())
            .estimate(),
        MemoryRow::new(
            module::SYNTAX_TREES,
            tr!("workbench-memory-syntax-trees"),
            Some(tree_bytes),
        )
        .count(tree_nodes)
        .estimate(),
        MemoryRow::new(
            module::WASM_STORES,
            tr!("workbench-memory-wasm-stores"),
            None,
        )
        .count(wasm.stores),
        MemoryRow::new(
            module::WASM_GRAMMARS,
            tr!("workbench-memory-wasm-grammars"),
            None,
        )
        .count(wasm.grammars),
        MemoryRow {
            count: symbols.map(|stats| stats.symbols as u64),
            ..MemoryRow::new(
                module::SYMBOL_INDEX,
                tr!("workbench-memory-symbol-index"),
                symbols.map(|stats| stats.approx_bytes as u64),
            )
            .estimate()
        },
        MemoryRow::new(
            module::PROJECT_TREE,
            tr!("workbench-memory-project-tree"),
            Some(tree.map_or(0, |tree| tree.heap_bytes_estimate() as u64)),
        )
        .count(tree.map_or(0, |tree| tree.stats().loaded_entries))
        .estimate(),
        MemoryRow {
            count: finder.map(|(paths, _)| paths as u64),
            ..MemoryRow::new(
                module::FINDER,
                tr!("workbench-memory-finder"),
                finder.map(|(_, bytes)| bytes as u64),
            )
            .estimate()
        },
        MemoryRow::new(
            module::FILE_VIEWS,
            tr!("workbench-memory-file-views"),
            Some(views.values().sum()),
        )
        .count(views.len()),
        MemoryRow::new(
            module::TERMINALS,
            tr!("workbench-memory-terminals"),
            scrollback,
        )
        .count(terminals)
        .estimate(),
        MemoryRow::new(module::LSP, tr!("workbench-memory-lsp"), None).count(documents),
        MemoryRow::new(module::MINIMAP, tr!("workbench-memory-minimap"), None),
    ]
}

/// The language servers running, each with its processes read by pid.
fn servers(workbench: &Workbench) -> Vec<ServerMemory> {
    let Some(manager) = workbench.lsp.manager() else {
        return Vec::new();
    };
    let running = (manager
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner))
    .running_servers();
    (running.into_iter())
        .map(|server| {
            let diagnostics = (workbench.lsp.file_diagnostics.iter())
                .filter(|((id, _), _)| *id == server.id)
                .map(|(_, (_, diagnostics))| diagnostics.len())
                .sum();
            ServerMemory {
                processes: server.pid.map(os::process_tree).unwrap_or_default(),
                memory_usage: None,
                diagnostics,
                id: server.id,
                instance: server.instance,
                generation: workbench.lsp.generation(),
                name: server.name,
                root: server.root,
                pid: server.pid,
                documents: server.documents,
            }
        })
        .collect()
}

/// rust-analyzer's `rust-analyzer/memoryUsage`: its own breakdown, as text.
enum MemoryUsage {}

impl lsp_types::request::Request for MemoryUsage {
    type Params = ();
    type Result = String;
    const METHOD: &'static str = MEMORY_USAGE;
}

/// The report tab's Restart lenses, one over each server's line.
struct RestartLenses(Vec<Lens>);

impl LensProvider for RestartLenses {
    fn lenses(&mut self, _: &fe_buffer::BufferSnapshot) -> Vec<Lens> {
        self.0.clone()
    }
}

/// The click on a server's Restart lens: `lsp.restart` for that instance,
/// named by its number and its manager's generation, which no other
/// instance of the window's has, now or after a new root: a root path
/// wouldn't survive the trip through JSON if it isn't UTF-8.
fn restart(server: &ServerMemory) -> Invocation {
    let args = [
        ("server".to_owned(), Value::from(server.name.as_str())),
        ("instance".to_owned(), Value::from(server.instance)),
        ("generation".to_owned(), Value::from(server.generation)),
    ];
    Invocation::of("lsp.restart")
        .expect("lsp.restart is registered")
        .args(Value::Object(args.into_iter().collect()))
}

impl Workbench {
    /// `memory.show`: the report in a new read-only tab, once rust-analyzer
    /// has given its breakdown ([`MemoryReport::collect_with_breakdowns`]).
    pub(crate) fn show_memory_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let report = MemoryReport::collect_with_breakdowns(self, cx);
        cx.spawn_in(window, async move |workbench, cx| {
            let report = report.await;
            (workbench.update_in(cx, |workbench, window, cx| {
                workbench.open_memory_report(&report, window, cx);
            }))
            .ok();
        })
        .detach();
    }

    fn open_memory_report(
        &mut self,
        report: &MemoryReport,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (text, server_rows) = report.render();
        let lenses = (server_rows.into_iter())
            .map(|(row, ix)| Lens {
                row,
                items: vec![LensItem {
                    title: tr!("workbench-memory-restart"),
                    invocation: restart(&report.servers[ix]),
                }],
            })
            .collect();
        let lenses: std::rc::Rc<std::cell::RefCell<dyn LensProvider>> =
            std::rc::Rc::new(std::cell::RefCell::new(RestartLenses(lenses)));
        let model = EditorModel::new(Buffer::from_file_contents(&text), None);
        let editor = self.pane().update(cx, |pane, cx| {
            let editor = pane.new_editor(model, cx);
            editor.update(cx, |editor, cx| {
                editor.set_read_only(true, cx);
                editor.set_tab_title(tr!("workbench-memory-tab-title"), "memory.txt", cx);
                editor.add_lens_provider(lenses);
            });
            pane.add_item(Box::new(editor.clone()), cx);
            editor
        });
        window.focus(&editor.focus_handle(cx), cx);
    }

    /// `lsp.restart` with the server instance the memory report's lens
    /// names: that one, whichever editor is active. One no longer running
    /// (it restarted since the report, or the root changed) is said to be
    /// gone.
    pub(crate) fn restart_server_instance(
        &mut self,
        name: &str,
        (generation, instance): (u64, u64),
        cx: &mut Context<Self>,
    ) {
        let manager = self.lsp.manager();
        // A manager of another root's numbers its instances afresh.
        let manager = manager.filter(|_| self.lsp.generation() == generation);
        let running = manager
            .map(|manager| {
                (manager
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner))
                .running_servers()
            })
            .unwrap_or_default();
        match running
            .into_iter()
            .find(|server| server.instance == instance)
        {
            Some(server) => self.restart_server(server.id, server.name, server.root, cx),
            None => toast::show(
                cx,
                Toast::info(tr!("workbench-memory-server-gone", name = name)),
            ),
        }
    }
}

/// Why a request got no answer, as the report shows it.
fn lsp_error(error: &LspError) -> String {
    match error {
        LspError::Timeout => tr!("workbench-memory-no-answer").to_string(),
        error => error.to_string(),
    }
}

/// What the OS says of this process and of others by pid.
#[cfg(target_os = "macos")]
mod os {
    use std::ffi::{c_int, c_void};

    use super::{ProcessMemory, ProcessUsage};

    /// `struct task_vm_info` (`mach/task_info.h`, packed to 4) as far as
    /// revision 5: the fields read are named.
    #[repr(C, packed(4))]
    #[derive(Default)]
    struct TaskVmInfo {
        virtual_size: u64,
        region_count: i32,
        page_size: i32,
        resident_size: u64,
        /// `resident_size_peak` to `compressed_lifetime`.
        _before_footprint: [u64; 15],
        phys_footprint: u64,
        /// `min_address` to `ledger_tag_media_nofootprint_compressed`.
        _before_graphics: [u64; 15],
        ledger_tag_graphics_footprint: i64,
        /// `ledger_tag_graphics_footprint_compressed` to
        /// `limit_bytes_remaining`.
        _rest: [u64; 8],
        decompressions: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct MallocStatistics {
        blocks_in_use: u32,
        size_in_use: usize,
        max_size_in_use: usize,
        size_allocated: usize,
    }

    const TASK_VM_INFO: u32 = 22;
    const PROC_PPID_ONLY: u32 = 6;

    unsafe extern "C" {
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
        fn malloc_zone_statistics(zone: *mut c_void, stats: *mut MallocStatistics);
        fn proc_listpids(kind: u32, info: u32, buffer: *mut c_void, size: c_int) -> c_int;
        fn proc_name(pid: c_int, buffer: *mut c_void, size: u32) -> c_int;
    }

    /// The `natural_t`s of `task_vm_info` up to the end of `field_end`
    /// bytes: what the kernel must have filled for a field to be read.
    const fn words(field_end: usize) -> u32 {
        field_end.div_ceil(4) as u32
    }

    pub(super) fn process() -> ProcessMemory {
        let mut info = TaskVmInfo::default();
        let mut count = words(size_of::<TaskVmInfo>());
        // SAFETY: `info` is `count` natural_t's long, and the kernel writes
        // at most that many, setting `count` to how many it did.
        let result = unsafe {
            task_info(
                mach_task_self_,
                TASK_VM_INFO,
                (&raw mut info).cast(),
                &mut count,
            )
        };
        let filled = |end: usize| result == 0 && count >= words(end);
        let footprint_end = std::mem::offset_of!(TaskVmInfo, phys_footprint) + 8;
        let graphics_end = std::mem::offset_of!(TaskVmInfo, ledger_tag_graphics_footprint) + 8;
        let mut stats = MallocStatistics::default();
        // SAFETY: a null zone asks for the sum over all zones.
        unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut stats) };
        ProcessMemory {
            footprint: filled(footprint_end).then_some(info.phys_footprint),
            graphics: filled(graphics_end)
                .then_some(info.ledger_tag_graphics_footprint.max(0) as u64),
            resident: (result == 0).then_some(info.resident_size),
            malloc_in_use: Some(stats.size_in_use as u64),
        }
    }

    /// `pid` and its descendants, each with its resident size and footprint
    /// from `proc_pid_rusage`, which needs no rights over another process
    /// of the same user, unlike `task_for_pid`.
    pub(super) fn process_tree(pid: u32) -> Vec<ProcessUsage> {
        let mut tree = Vec::new();
        visit(pid, 0, &mut tree);
        tree
    }

    fn visit(pid: u32, depth: usize, tree: &mut Vec<ProcessUsage>) {
        let (resident, footprint) = usage(pid);
        tree.push(ProcessUsage {
            pid,
            name: name(pid),
            depth,
            resident,
            footprint,
        });
        if depth < super::MAX_DEPTH {
            for child in children(pid) {
                visit(child, depth + 1, tree);
            }
        }
    }

    fn usage(pid: u32) -> (Option<u64>, Option<u64>) {
        // SAFETY: `info` is a `rusage_info_v4`, the flavour asked for.
        let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::proc_pid_rusage(
                pid as c_int,
                libc::RUSAGE_INFO_V4,
                (&raw mut info).cast::<libc::rusage_info_t>(),
            )
        };
        match result {
            0 => (Some(info.ri_resident_size), Some(info.ri_phys_footprint)),
            _ => (None, None),
        }
    }

    fn children(pid: u32) -> Vec<u32> {
        let mut pids = [0 as c_int; 256];
        // SAFETY: the buffer is as long as said. Whether the result counts
        // bytes or pids has varied, so the zeroed buffer's entries are read
        // instead.
        unsafe {
            proc_listpids(
                PROC_PPID_ONLY,
                pid,
                pids.as_mut_ptr().cast(),
                size_of_val(&pids) as c_int,
            )
        };
        (pids.into_iter())
            .filter(|&child| child > 0)
            .map(|child| child as u32)
            .collect()
    }

    fn name(pid: u32) -> Option<String> {
        let mut buffer = [0u8; 256];
        // SAFETY: the buffer is as long as said.
        let len = unsafe { proc_name(pid as c_int, buffer.as_mut_ptr().cast(), 256) };
        (len > 0).then(|| String::from_utf8_lossy(&buffer[..len as usize]).into_owned())
    }
}

#[cfg(target_os = "linux")]
mod os {
    use std::{fs, path::PathBuf};

    use kiln_l10n::verbatim;

    use super::{ProcessMemory, ProcessUsage};

    /// `/proc/<pid>/<file>`.
    fn proc(pid: u32, file: &str) -> PathBuf {
        PathBuf::from("/proc").join(pid.to_string()).join(file)
    }

    /// `status`'s field `name` in bytes: `VmRSS:     123456 kB`.
    fn kilobytes(status: &str, name: &str) -> Option<u64> {
        let rest = status.lines().find_map(|line| line.strip_prefix(name))?;
        let value = rest.trim().strip_suffix("kB")?;
        Some(value.trim().parse::<u64>().ok()? * 1024)
    }

    pub(super) fn process() -> ProcessMemory {
        let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
        ProcessMemory {
            footprint: None,
            graphics: None,
            resident: kilobytes(&status, verbatim!("VmRSS:")),
            malloc_in_use: malloc_in_use(),
        }
    }

    /// glibc's chunks in use: `uordblks` from its heap arenas, and
    /// `hblkhd`, the large ones it gives their own mappings.
    #[cfg(target_env = "gnu")]
    fn malloc_in_use() -> Option<u64> {
        #[repr(C)]
        struct Mallinfo2 {
            arena: usize,
            ordblks: usize,
            smblks: usize,
            hblks: usize,
            hblkhd: usize,
            usmblks: usize,
            fsmblks: usize,
            uordblks: usize,
            fordblks: usize,
            keepcost: usize,
        }
        unsafe extern "C" {
            fn mallinfo2() -> Mallinfo2;
        }
        // SAFETY: mallinfo2 takes nothing and returns its struct by value
        // (glibc 2.33 and later).
        let info = unsafe { mallinfo2() };
        Some((info.uordblks + info.hblkhd) as u64)
    }

    #[cfg(not(target_env = "gnu"))]
    fn malloc_in_use() -> Option<u64> {
        None
    }

    /// `pid` and its descendants, from `/proc`.
    pub(super) fn process_tree(pid: u32) -> Vec<ProcessUsage> {
        let parents = parents();
        let mut tree = Vec::new();
        visit(pid, 0, &parents, &mut tree);
        tree
    }

    fn visit(pid: u32, depth: usize, parents: &[(u32, u32)], tree: &mut Vec<ProcessUsage>) {
        let status = fs::read_to_string(proc(pid, "status")).unwrap_or_default();
        let name = (status.lines())
            .find_map(|line| line.strip_prefix("Name:"))
            .map(|name| name.trim().to_owned());
        let anonymous = kilobytes(&status, verbatim!("RssAnon:"));
        let swapped = kilobytes(&status, verbatim!("VmSwap:")).unwrap_or(0);
        tree.push(ProcessUsage {
            pid,
            name,
            depth,
            resident: kilobytes(&status, verbatim!("VmRSS:")),
            footprint: anonymous.map(|anonymous| anonymous + swapped),
        });
        if depth < super::MAX_DEPTH {
            for &(child, _) in parents.iter().filter(|(_, parent)| *parent == pid) {
                visit(child, depth + 1, parents, tree);
            }
        }
    }

    /// Every process and its parent, from each `/proc/<pid>/stat`.
    fn parents() -> Vec<(u32, u32)> {
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        (entries.flatten())
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| {
                let stat = fs::read_to_string(proc(pid, "stat")).ok()?;
                // `pid (comm) state ppid …`: comm may hold spaces and parens.
                let after = &stat[stat.rfind(')')? + 1..];
                let parent = after.split_whitespace().nth(1)?.parse().ok()?;
                Some((pid, parent))
            })
            .collect()
    }
}

#[cfg(windows)]
mod os {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use super::{ProcessMemory, ProcessUsage};

    /// The working set and the private bytes committed of the process
    /// `handle`.
    fn counters(handle: windows_sys::Win32::Foundation::HANDLE) -> Option<(u64, u64)> {
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        // SAFETY: `counters` is valid for writes of the size given.
        let ok = unsafe { GetProcessMemoryInfo(handle, &mut counters, counters.cb) };
        (ok != 0).then_some((
            counters.WorkingSetSize as u64,
            counters.PagefileUsage as u64,
        ))
    }

    pub(super) fn process() -> ProcessMemory {
        // SAFETY: the pseudo-handle is always valid.
        let resident = counters(unsafe { GetCurrentProcess() }).map(|(resident, _)| resident);
        ProcessMemory {
            footprint: None,
            graphics: None,
            resident,
            malloc_in_use: None,
        }
    }

    /// `pid` and its descendants, from a Toolhelp snapshot: each one's
    /// working set, and its private bytes as its footprint.
    pub(super) fn process_tree(pid: u32) -> Vec<ProcessUsage> {
        let processes = processes();
        let mut tree = Vec::new();
        visit(pid, 0, &processes, &mut tree);
        tree
    }

    fn visit(
        pid: u32,
        depth: usize,
        processes: &[(u32, u32, String)],
        tree: &mut Vec<ProcessUsage>,
    ) {
        // SAFETY: a handle opened is closed below.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        let usage = if handle.is_null() {
            None
        } else {
            let usage = counters(handle);
            // SAFETY: opened above.
            unsafe { CloseHandle(handle) };
            usage
        };
        let name = (processes.iter())
            .find(|(of, _, _)| *of == pid)
            .map(|(_, _, name)| name.clone());
        tree.push(ProcessUsage {
            pid,
            name,
            depth,
            resident: usage.map(|(resident, _)| resident),
            footprint: usage.map(|(_, private)| private),
        });
        if depth < super::MAX_DEPTH {
            for (child, _, _) in processes.iter().filter(|(_, parent, _)| *parent == pid) {
                visit(*child, depth + 1, processes, tree);
            }
        }
    }

    /// Every process, its parent and its executable's name.
    fn processes() -> Vec<(u32, u32, String)> {
        // SAFETY: a snapshot of every process; closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot.is_null() || snapshot as isize == -1 {
            return Vec::new();
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        let mut processes = Vec::new();
        // SAFETY: `entry` is a PROCESSENTRY32W with its size set.
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while more {
            let len = (entry.szExeFile.iter())
                .position(|&unit| unit == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            processes.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
            // SAFETY: as above.
            more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        // SAFETY: opened above.
        unsafe { CloseHandle(snapshot) };
        processes
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod os {
    use super::{ProcessMemory, ProcessUsage};

    pub(super) fn process() -> ProcessMemory {
        ProcessMemory::default()
    }

    pub(super) fn process_tree(pid: u32) -> Vec<ProcessUsage> {
        vec![ProcessUsage {
            pid,
            name: None,
            depth: 0,
            resident: None,
            footprint: None,
        }]
    }
}

/// How far below a server its descendants are listed.
const MAX_DEPTH: usize = 3;
