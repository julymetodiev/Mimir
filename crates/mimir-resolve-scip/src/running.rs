//! The servers running for a project, one per root, stopped together when unused.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Servers by the root each was started in.
pub struct Running<S> {
    running: HashMap<PathBuf, S>,
    last_used: Option<Instant>,
}

impl<S> Default for Running<S> {
    fn default() -> Self {
        Self {
            running: HashMap::new(),
            last_used: None,
        }
    }
}

impl<S> Running<S> {
    /// Stop every server if none was used for `idle`, and say how many that was.
    pub fn stop_if_idle(&mut self, idle: Duration) -> Option<usize> {
        let unused = self.last_used.is_some_and(|used| used.elapsed() >= idle);
        if !unused || self.running.is_empty() {
            return None;
        }
        let stopped = self.running.len();
        self.running.clear();
        self.last_used = None;
        Some(stopped)
    }

    /// Ask the server of a root, starting it if none runs. One that fails is not kept.
    pub fn with<T, E>(
        &mut self,
        root: &Path,
        start: impl FnOnce() -> Result<S, E>,
        ask: impl FnOnce(&mut S) -> Result<T, E>,
    ) -> Result<T, E> {
        self.last_used = Some(Instant::now());
        let server = match self.running.entry(root.to_path_buf()) {
            std::collections::hash_map::Entry::Occupied(found) => found.into_mut(),
            std::collections::hash_map::Entry::Vacant(place) => place.insert(start()?),
        };
        let result = ask(server);
        if result.is_err() {
            self.running.remove(root);
        }
        self.last_used = Some(Instant::now());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_is_kept_until_it_fails_or_is_idle() {
        let mut running: Running<u32> = Running::default();
        let root = Path::new("/a");
        assert_eq!(
            running.with::<_, ()>(root, || Ok(1), |server| Ok(*server)),
            Ok(1)
        );
        assert_eq!(
            running.with::<_, ()>(root, || Ok(2), |server| Ok(*server)),
            Ok(1),
            "the first is still there"
        );
        assert_eq!(running.with(root, || Ok(3), |_| Err::<u32, _>(())), Err(()));
        assert_eq!(
            running.with::<_, ()>(root, || Ok(4), |server| Ok(*server)),
            Ok(4)
        );
        assert_eq!(running.stop_if_idle(Duration::from_secs(600)), None);
        assert_eq!(running.stop_if_idle(Duration::ZERO), Some(1));
        assert_eq!(running.stop_if_idle(Duration::ZERO), None);
    }
}
