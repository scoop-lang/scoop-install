use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

const BAR_WIDTH: usize = 20;
const REFRESH_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) struct DownloadProgress {
    total: u64,
    downloaded: u64,
    started: Instant,
    last_draw: Instant,
    line_width: usize,
    interactive: bool,
    finished: bool,
}

impl DownloadProgress {
    pub(crate) fn new(total: u64) -> Self {
        let now = Instant::now();
        let mut progress = Self {
            total,
            downloaded: 0,
            started: now,
            last_draw: now,
            line_width: 0,
            interactive: io::stderr().is_terminal()
                && std::env::var("TERM").as_deref() != Ok("dumb"),
            finished: false,
        };
        progress.draw();
        progress
    }

    pub(crate) fn set_position(&mut self, downloaded: u64) {
        self.downloaded = downloaded;
        if self.last_draw.elapsed() >= REFRESH_INTERVAL {
            self.draw();
        }
    }

    pub(crate) fn finish(mut self) {
        self.finished = true;
        if !self.interactive {
            eprintln!("Downloaded {} bytes", self.downloaded);
        }
    }

    fn draw(&mut self) {
        if !self.interactive {
            return;
        }
        let percent = if self.finished {
            100
        } else if self.total == 0 {
            0
        } else {
            // Reserve 100% for a verified archive committed to the cache.
            ((self.downloaded as f64 / self.total as f64) * 100.0).min(99.0) as usize
        };
        let filled = percent * BAR_WIDTH / 100;
        let speed = self.downloaded as f64 / self.started.elapsed().as_secs_f64().max(0.001);
        let line = format!(
            "  [{}{}] {percent:>3}% {}/{} {}/s",
            "=".repeat(filled),
            " ".repeat(BAR_WIDTH - filled),
            bytes(self.downloaded as f64),
            bytes(self.total as f64),
            bytes(speed),
        );
        let mut stderr = io::stderr().lock();
        let width = self.line_width.max(line.len());
        let _ = write!(stderr, "\r{line:width$}");
        let _ = stderr.flush();
        self.line_width = line.len();
        self.last_draw = Instant::now();
    }
}

impl Drop for DownloadProgress {
    fn drop(&mut self) {
        if self.interactive {
            self.draw();
            let _ = writeln!(io::stderr());
        }
    }
}

fn bytes(mut value: f64) -> String {
    for unit in ["B", "KiB", "MiB", "GiB", "TiB"] {
        if value < 1024.0 || unit == "TiB" {
            return format!("{value:.1} {unit}");
        }
        value /= 1024.0;
    }
    unreachable!()
}
