//! Config change detection via mtime + size (T031, FR-016).

use std::path::Path;

/// Watches one config file; each `changed()` call compares the current
/// mtime + size against the last observation.
#[derive(Debug, Clone, Default)]
pub struct ConfigWatch {
    last: Option<(std::time::SystemTime, u64)>,
}

impl ConfigWatch {
    /// Observe the file now; returns true when it changed since the previous
    /// observation. The first observation reports no change.
    pub fn changed(&mut self, path: &Path) -> anyhow::Result<bool> {
        let meta = std::fs::metadata(path)?;
        let mtime = meta.modified()?;
        let size = meta.len();
        let changed = self.last.is_some_and(|(m, s)| m != mtime || s != size);
        self.last = Some((mtime, size));
        Ok(changed)
    }

    /// Forget the last observation so the next `changed()` reports no change
    /// (used after a deliberate reload).
    pub fn reset(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_modification() {
        let dir = std::env::temp_dir().join(format!("sb-tui-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.json");
        std::fs::write(&file, "{}").unwrap();

        let mut w = ConfigWatch::default();
        assert!(
            !w.changed(&file).unwrap(),
            "first observation is not a change"
        );
        assert!(!w.changed(&file).unwrap(), "untouched file is not a change");

        // Bump mtime + size.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        writeln!(f, r#" "x": 1"#).unwrap();
        drop(f);

        assert!(w.changed(&file).unwrap(), "modified file must be detected");
        assert!(
            !w.changed(&file).unwrap(),
            "same state again is not a change"
        );

        w.reset();
        assert!(!w.changed(&file).unwrap(), "reset forgets the baseline");
        std::fs::remove_dir_all(&dir).ok();
    }
}
