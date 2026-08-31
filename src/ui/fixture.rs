//! The frontend-active reference state from the accepted design export.
//!
//! Fixtures build real [`FakeEngine`] state so the renderer only ever reads
//! terminal cells, cursor, status, and metadata through the frozen contracts.

use std::path::{Path, PathBuf};

use crate::{
    contracts::{
        CellContent, CellStyle, CellWidth, Elapsed, ProcessInfo, Project, Rgb, ScreenCell,
        ScreenSize, TerminalFrame, TerminalId, TerminalMetadata, TerminalStatus, Timestamp,
    },
    engine::FakeEngine,
};

/// Home directory the reference paths are abbreviated against.
pub const HOME: &str = "/home/dev";
/// Wall clock the reference state is rendered at: 12:06:14 UTC.
pub const NOW: Timestamp = Timestamp {
    unix_millis: 43_574_000,
};

const MASTER: ScreenSize = ScreenSize::new(92, 38);
const PREVIEW: ScreenSize = ScreenSize::new(38, 10);

const FRONTEND: &[&str] = &[
    "$ pnpm dev",
    "",
    "> idp-frontend@0.4.2 dev",
    "> vite --host",
    "",
    "  VITE v5.4.8  ready in 412 ms",
    "",
    "  ➜  Local:   http://localhost:5173/",
    "  ➜  Network: http://192.168.1.24:5173/",
    "  ➜  press h + enter to show help",
    "",
    "12:03:58 [vite] page reload src/main.tsx",
    "12:04:12 [vite] hmr update /src/routes/WorkspaceList.tsx",
    "12:04:12 [vite] hmr update /src/styles/tokens.css",
    "12:04:29 [vite] hmr update /src/routes/WorkspaceList.tsx",
    "12:04:41 [vite] optimized dependencies changed, reloading",
    "",
    "  VITE v5.4.8  ready in 268 ms",
    "",
    "12:05:02 [vite] hmr update /src/components/PaneTitle.tsx",
    "12:05:09 warning  src/components/PaneTitle.tsx:17:8",
    "         'status' is declared but its value is never read.",
    "12:05:17 [vite] hmr update /src/components/PaneTitle.tsx",
    "12:05:33 [vite] hmr update /src/components/StatusBar.tsx",
    "12:05:58 [vite] hmr update /src/routes/Dashboard.tsx",
    "12:06:04 [vite] hmr invalidate /src/hooks/useTerminals.ts — could not Fast Refresh",
    "12:06:04 [vite] page reload src/hooks/useTerminals.ts",
    "12:06:11 [vite] hmr update /src/routes/Dashboard.tsx",
];

const BACKEND: &[&str] = &[
    "  INFO  Server running on",
    "        [http://127.0.0.1:8000]",
    "",
    "12:05:52 /api/workspaces ····· 42.1ms",
    "12:05:53 /api/workspaces/idp/… 118ms",
    "12:05:58 /api/terminals ······ 11.4ms",
    "12:06:01 /api/health ·········  3.0ms",
    "12:06:07 queue ProcessLog ··· done",
    "12:06:09 /api/terminals/2/log  8.7ms",
];

const APP: &[&str] = &[
    "> idp-app@0.2.1 dev",
    "> expo start --dev-client",
    "",
    "✖ Metro bundler failed to start",
    "  Error: EADDRINUSE :::8081",
    "  another process is bound to",
    "  port 8081",
];

const WORKER: &[&str] = &[
    "  INFO  Processing jobs from the",
    "        [default] queue.",
    "",
    "12:00:14 SyncRepos ········ DONE",
    "12:00:19 IndexWorkspace ··· DONE",
    "12:00:21 SendDigest ······· DONE",
    "",
    "no output for 6m",
];

/// The four configured projects of the `idp` workspace.
pub fn projects() -> Vec<Project> {
    [
        ("frontend", "idp/frontend", ["pnpm", "dev"].as_slice()),
        ("backend", "idp/backend", ["uv", "run", "api"].as_slice()),
        ("app", "idp/app", ["pnpm", "start"].as_slice()),
        ("worker", "idp/backend", ["uv", "run", "worker"].as_slice()),
    ]
    .into_iter()
    .map(|(name, path, command)| Project {
        terminal: TerminalId::new(name),
        path: PathBuf::from(HOME).join(path),
        command: command.iter().map(|part| (*part).to_owned()).collect(),
    })
    .collect()
}

/// Engine state for reference screen 01: frontend master, backend running,
/// app exited with code 1, worker running but idle.
pub fn frontend_active() -> FakeEngine {
    let projects = projects();
    let mut engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));

    let frontend = TerminalId::new("frontend");
    let mut frame = screen(&frontend, MASTER, FRONTEND);
    frame.cursor.row = FRONTEND.len() as u16;
    // Two coloured runs prove engine styles survive into the buffer.
    paint(&mut frame, 5, 2, 11, bold(0xc8, 0x98, 0xe0));
    paint(&mut frame, 7, 13, 22, plain(0x7a, 0xa2, 0xf7));
    engine.set_frame(frame);
    engine.set_status(&frontend, TerminalStatus::Running);
    engine.set_metadata(&frontend, running(12_000, 4_207_331, 41_233));

    let backend = TerminalId::new("backend");
    engine.set_frame(screen(&backend, PREVIEW, BACKEND));
    engine.set_status(&backend, TerminalStatus::Running);
    engine.set_metadata(&backend, running(22_000, 91_204, 41_240));

    let app = TerminalId::new("app");
    engine.set_frame(screen(&app, PREVIEW, APP));
    engine.set_status(&app, TerminalStatus::Exited { code: Some(1) });
    engine.set_metadata(
        &app,
        TerminalMetadata {
            bytes_written: 3_118,
            output_idle: Some(Elapsed { millis: 132_000 }),
            // 12:04:02 UTC, two minutes before NOW.
            last_exit_at: Some(Timestamp {
                unix_millis: NOW.unix_millis - 132_000,
            }),
            ..TerminalMetadata::default()
        },
    );

    let worker = TerminalId::new("worker");
    engine.set_frame(screen(&worker, PREVIEW, WORKER));
    engine.set_status(&worker, TerminalStatus::Running);
    engine.set_metadata(&worker, running(384_000, 2_044, 41_255));

    engine
}

pub fn home() -> &'static Path {
    Path::new(HOME)
}

fn running(idle_millis: u64, bytes_written: u64, pid: u32) -> TerminalMetadata {
    TerminalMetadata {
        bytes_written,
        output_idle: Some(Elapsed {
            millis: idle_millis,
        }),
        process: Some(ProcessInfo {
            pid,
            uptime: Elapsed { millis: 600_000 },
        }),
        ..TerminalMetadata::default()
    }
}

fn screen(terminal: &TerminalId, size: ScreenSize, lines: &[&str]) -> TerminalFrame {
    let mut frame = TerminalFrame::blank(terminal.clone(), size, 1);
    for (row, line) in lines.iter().enumerate().take(size.rows as usize) {
        for (column, character) in line.chars().enumerate().take(size.columns as usize) {
            let Some(index) = frame.cell_index(column as u16, row as u16) else {
                continue;
            };
            frame.cells[index] = ScreenCell {
                content: CellContent::Glyph {
                    text: character.to_string(),
                    width: CellWidth::One,
                },
                style: CellStyle::default(),
            };
        }
    }
    frame.cursor.column = 0;
    frame
}

fn paint(frame: &mut TerminalFrame, row: u16, from: u16, to: u16, style: CellStyle) {
    for column in from..to {
        if let Some(index) = frame.cell_index(column, row) {
            frame.cells[index].style = style;
        }
    }
}

const fn plain(red: u8, green: u8, blue: u8) -> CellStyle {
    CellStyle {
        foreground: Some(Rgb { red, green, blue }),
        ..const_style()
    }
}

const fn bold(red: u8, green: u8, blue: u8) -> CellStyle {
    CellStyle {
        bold: true,
        ..plain(red, green, blue)
    }
}

const fn const_style() -> CellStyle {
    CellStyle {
        foreground: None,
        background: None,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
        inverse: false,
    }
}
