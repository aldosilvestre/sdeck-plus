use ksni::{Tray, TrayMethods};
use std::{thread, time::Duration};

pub(super) const GRACE: Duration = Duration::from_secs(10);
type Receipt<T> = async_channel::Receiver<Result<ksni::Handle<T>, String>>;

/// Owns the runtime as well as registration: dropping it cancels pending D-Bus work.
pub(super) struct Service {
    stop: async_channel::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Service {
    pub(super) fn start<T: Tray + Clone>(
        tray: T,
        grace: Duration,
    ) -> std::io::Result<(Self, Receipt<T>)> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (stop, stopped) = async_channel::bounded(1);
        let (sender, receiver) = async_channel::bounded(1);
        let worker = thread::Builder::new().name("tray-startup".into()).spawn(move || {
            runtime.block_on(async move {
                tokio::select! {
                    _ = stopped.recv() => {},
                    result = tokio::time::timeout(grace, async {
                        loop {
                            // Default spawn checks registration AND host availability.
                            if let Ok(Ok(handle)) = tokio::time::timeout(
                                Duration::from_millis(500), tray.clone().spawn(),
                            ).await {
                                break handle;
                            }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }) => {
                        match result {
                            Ok(handle) => {
                                if sender.send(Ok(handle)).await.is_ok() {
                                    let _ = stopped.recv().await;
                                }
                            }
                            Err(_) => {
                                let _ = sender.send(Err("No usable StatusNotifier tray registered before the startup deadline".into())).await;
                            }
                        }
                    }
                }
            });
        })?;
        Ok((
            Self {
                stop,
                worker: Some(worker),
            },
            receiver,
        ))
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.stop.try_send(());
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                eprintln!("Tray startup worker panicked");
            }
        }
    }
}
