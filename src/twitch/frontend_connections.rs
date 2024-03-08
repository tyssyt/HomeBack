
use super::twitch_auth::{AuthorizationRequest, Authorization};

use std::sync::Mutex;
use std::time::Instant;
use uuid::Uuid;

pub struct FrontendConnections {
    pending: Mutex<Vec<Pending>>,
    logged_in: Mutex<Vec<LoggedIn>>, // TODO think about how/when to remove from this list
}

struct Pending {
    id: Uuid,
    created_at: Instant,
    auth_request: AuthorizationRequest,
}

struct LoggedIn {
    id: Uuid,
    auth: Authorization,
}

impl FrontendConnections {

    pub fn new() -> Self {
        Self { pending: Mutex::from(Vec::new()), logged_in: Mutex::from(Vec::new()) }
    }

    pub fn create(&self, auth_request: AuthorizationRequest) -> Uuid {
        self.clean_pending();
        
        let id = Uuid::new_v4();
        let mut pending = self.pending.lock().unwrap();
        pending.push(Pending{id, created_at: Instant::now(), auth_request});
        id
    }

    pub fn get_pending(&self, id: &Uuid) -> Option<(String, String)> {
        self.clean_pending();   
        let pending = self.pending.lock().unwrap();
        pending.iter()
            .find(|login| login.id == *id)
            .map(|login| (login.auth_request.device_code.clone(), login.auth_request.verification_uri.clone()))
    }

    pub fn log_in(&self, id: Uuid, auth: Authorization) {
        self.remove(&id);
        let mut logged_in = self.logged_in.lock().unwrap();
        logged_in.push( LoggedIn{id, auth} );
    }

    pub fn get_logged_in(&self, id: &Uuid) -> Option<(String, String)> {
        let logged_in = self.logged_in.lock().unwrap();
        logged_in.iter().find(|login| login.id == *id).map(|login| (login.auth.access_token.clone(), login.auth.refresh_token.clone()))
    }

    pub fn update_logged_in(&self, id: &Uuid, auth: Authorization) -> Option<()> {
        let mut logged_in = self.logged_in.lock().unwrap();
        let i = logged_in.iter().position(|login| login.id == *id)?;
        logged_in[i].auth = auth;
        Some(())
    }

    pub fn remove(&self, id: &Uuid) {
        {
            let mut pending = self.pending.lock().unwrap();
            pending.retain(|login| login.id != *id);
        }
        {
            let mut logged_in = self.logged_in.lock().unwrap();
            logged_in.retain(|login| login.id != *id);
        }
    }

    fn clean_pending(&self) {
        let mut pending_logins = self.pending.lock().unwrap();
        pending_logins.retain(|login| login.created_at.elapsed().as_secs() < login.auth_request.expires_in)
    }


}