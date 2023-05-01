use futures::stream::FuturesUnordered;
use futures::StreamExt;
use rattler_conda_types::PrefixRecord;
use std::path::PathBuf;
use tokio::task::JoinHandle;

/// Points to a directory that serves as a Conda prefix.
pub struct Prefix {
    root: PathBuf,
}

impl Prefix {
    /// Constructs a new instance. Returns an error if the directory doesnt exist.
    pub fn new(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let root = path.into();
        Ok(Self { root })
    }

    /// Scans the `conda-meta` directory of an environment and returns all the [`PrefixRecord`]s found
    /// in there.
    pub async fn find_installed_packages(
        &self,
        concurrency_limit: Option<usize>,
    ) -> anyhow::Result<Vec<PrefixRecord>> {
        let concurrency_limit = concurrency_limit.unwrap_or(100);
        let mut meta_futures =
            FuturesUnordered::<JoinHandle<Result<PrefixRecord, std::io::Error>>>::new();
        let mut result = Vec::new();
        for entry in std::fs::read_dir(self.root.join("conda-meta"))
            .into_iter()
            .flatten()
        {
            let entry = entry?;
            let path = entry.path();
            if path.ends_with(".json") {
                continue;
            }

            // If there are too many pending entries, wait for one to be finished
            if meta_futures.len() >= concurrency_limit {
                match meta_futures
                    .next()
                    .await
                    .expect("we know there are pending futures")
                {
                    Ok(record) => result.push(record?),
                    Err(e) => {
                        if let Ok(panic) = e.try_into_panic() {
                            std::panic::resume_unwind(panic);
                        }
                        // The future was cancelled, we can simply return what we have.
                        return Ok(result);
                    }
                }
            }

            // Spawn loading on another thread
            let future = tokio::task::spawn_blocking(move || PrefixRecord::from_path(path));
            meta_futures.push(future);
        }

        while let Some(record) = meta_futures.next().await {
            match record {
                Ok(record) => result.push(record?),
                Err(e) => {
                    if let Ok(panic) = e.try_into_panic() {
                        std::panic::resume_unwind(panic);
                    }
                    // The future was cancelled, we can simply return what we have.
                    return Ok(result);
                }
            }
        }

        Ok(result)
    }
}