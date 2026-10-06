use std::{collections::HashMap, sync::Mutex};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

struct ActiveRun {
    id: String,
    cancel: CancellationToken,
    approvals: HashMap<String, oneshot::Sender<bool>>,
}
#[derive(Default)]
pub struct Runs {
    active: Mutex<Option<ActiveRun>>,
}
impl Runs {
    pub fn is_active(&self) -> bool {
        self.active
            .lock()
            .map(|active| active.is_some())
            .unwrap_or(true)
    }
    pub fn begin(&self, id: &str) -> Result<CancellationToken, String> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid run ID.".to_string())?;
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Run lock failed.".to_string())?;
        if active.is_some() {
            return Err("A response is already running. Stop it before starting another.".into());
        }
        let cancel = CancellationToken::new();
        *active = Some(ActiveRun {
            id: id.into(),
            cancel: cancel.clone(),
            approvals: HashMap::new(),
        });
        Ok(cancel)
    }
    pub fn finish(&self, id: &str) {
        if let Ok(mut active) = self.active.lock() {
            if active.as_ref().is_some_and(|r| r.id == id) {
                *active = None;
            }
        }
    }
    pub fn stop(&self, id: &str) -> Result<(), String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Run lock failed.".to_string())?;
        if let Some(run) = active.as_mut().filter(|r| r.id == id) {
            run.cancel.cancel();
            run.approvals.clear();
        }
        Ok(())
    }
    pub fn stop_all(&self) {
        if let Ok(mut active) = self.active.lock() {
            if let Some(run) = active.as_mut() {
                run.cancel.cancel();
                run.approvals.clear();
            }
        }
    }
    pub fn pending(&self, id: &str) -> Result<(String, oneshot::Receiver<bool>), String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Run lock failed.".to_string())?;
        let run = active
            .as_mut()
            .filter(|r| r.id == id && !r.cancel.is_cancelled())
            .ok_or_else(|| "The run is no longer active.".to_string())?;
        let approval_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        run.approvals.insert(approval_id.clone(), tx);
        Ok((approval_id, rx))
    }
    pub fn approve(&self, id: &str, approval_id: &str, allow: bool) -> Result<(), String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Run lock failed.".to_string())?;
        let run = active
            .as_mut()
            .filter(|r| r.id == id && !r.cancel.is_cancelled())
            .ok_or_else(|| "This approval belongs to an inactive run.".to_string())?;
        let tx = run
            .approvals
            .remove(approval_id)
            .ok_or_else(|| "This approval expired or was already answered.".to_string())?;
        tx.send(allow)
            .map_err(|_| "This approval expired.".to_string())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn approvals_are_one_use_and_run_bound() {
        let runs = Runs::default();
        let id = uuid::Uuid::new_v4().to_string();
        runs.begin(&id).unwrap();
        let (aid, rx) = runs.pending(&id).unwrap();
        assert!(runs.approve("wrong", &aid, true).is_err());
        runs.approve(&id, &aid, true).unwrap();
        assert!(rx.await.unwrap());
        assert!(runs.approve(&id, &aid, true).is_err());
    }
    #[tokio::test]
    async fn stop_revokes_pending_and_late_events_cannot_stop_new_run() {
        let runs = Runs::default();
        let id = uuid::Uuid::new_v4().to_string();
        let token = runs.begin(&id).unwrap();
        let (a, rx) = runs.pending(&id).unwrap();
        runs.stop(&id).unwrap();
        assert!(token.is_cancelled());
        assert!(rx.await.is_err());
        assert!(runs.approve(&id, &a, true).is_err());
        runs.finish(&id);
        let next = uuid::Uuid::new_v4().to_string();
        let token = runs.begin(&next).unwrap();
        runs.stop(&id).unwrap();
        runs.finish(&id);
        assert!(!token.is_cancelled());
        assert!(runs.begin(&id).is_err());
    }
}
