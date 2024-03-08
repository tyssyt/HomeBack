mod frontend_connections;
use frontend_connections::*;
mod twitch_auth;
use twitch_auth::*;
mod twitch_follows;
use twitch_follows::*;

use std::env;
use uuid::Uuid;
use log::info;
use itertools::Itertools;
use serde::{Serialize, Deserialize};

pub struct Twitch {
    connections: FrontendConnections,
    auth_client: TwitchAuthClient,
    follows: TwitchFollows,
}

#[derive(Serialize, Debug)]
pub struct LoginResponse {
    id: Uuid,
    logged_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification_uri: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct FollowResponse {
    profile_image_url: String,
    offline_image_url: String,
    #[serde(flatten)]
    stream: Stream,
}

#[derive(Deserialize, Debug)]
struct Data<T> {
    data: Vec<T>,
}

#[derive(Deserialize, Debug)]
struct PagedData<T> {
    data: Vec<T>,    
    pagination: Pagination,
}

#[derive(Deserialize, Debug)]
struct Pagination {
    cursor: Option<String>,
}

impl Twitch {

    pub fn new() -> Self {
        let client_id: String = env::var("TWITCH_CLIENT_ID").expect("TWITCH_CLIENT_ID not set");
        let client_secret = env::var("TWITCH_CLIENT_SECRET").expect("TWITCH_CLIENT_SECRET not set");
        return Self {connections: FrontendConnections::new(), follows: TwitchFollows::new(&client_id), auth_client: TwitchAuthClient::new(client_id, client_secret)};
    }

    pub fn create_user_login(&self) -> Result<LoginResponse, reqwest::Error> {
        let auth_request = self.auth_client.create_authorization_request()?;
        let verification_uri = auth_request.verification_uri.clone();
        let id = self.connections.create(auth_request);

        let login_response = LoginResponse { id, logged_in: false, verification_uri: Some(verification_uri) };
        info!("Starting User Login: {:?}", &login_response);
        Ok(login_response)
    }

    pub fn get_user_login(&self, id: Uuid) -> Option<LoginResponse> {
        self.get_user_login_from_pending(id)
        .or_else(|| self.get_valid_access_token(&id).map(|_token| LoginResponse{id, logged_in: true, verification_uri: None}))
    }

    fn get_user_login_from_pending(&self, id: Uuid) -> Option<LoginResponse> {
        let (device_code, verification_uri) = self.connections.get_pending(&id)?;
   
        match self.auth_client.activate_authorization_request(&device_code) {
            Ok(Some(auth)) => {
                info!("User Authentication Successful: {:?}", &auth);
                self.connections.log_in(id, auth);
                Some(LoginResponse { id, logged_in: true, verification_uri: None })
            },
            Ok(None) =>  Some(LoginResponse{id, logged_in: false, verification_uri: Some(verification_uri)}),
            Err(_) => None, // in theory we could also delete the pending login here, but that's not worth the effort
        }
     }

     fn get_valid_access_token(&self, id: &Uuid) -> Option<(String, Validation)> {
         let (access_token, refresh_token) = self.connections.get_logged_in(id)?;
         let valid_token = self.validate_token(id, access_token, refresh_token);
         if valid_token.is_none() {
            info!("User Authentication for {} has become invalid", id);
            self.connections.remove(id);
         }
         valid_token
     }

     fn validate_token(&self, id: &Uuid, access_token: String, refresh_token: String) -> Option<(String, Validation)> {        
        match self.auth_client.validate_authorization(&access_token) {
            Ok(Some(validation)) => Some((access_token, validation)),
            Ok(None) => {
                info!("User Authentication for {}/{} expired, attempting refresh", id, &access_token);
                let new_auth = self.auth_client.refresh_authorization(&refresh_token).ok()?;
                let new_token = new_auth.access_token.clone();
                let validation = self.auth_client.validate_authorization(&new_token).ok()??;

                info!("User Authentication Refresh Successful: {}/{:?}", &id, &new_auth);
                self.connections.update_logged_in(id, new_auth)?;
                Some((new_token, validation))
            },
            Err(_) => None,
        }
     }

    pub fn get_online_following(&self, id: Uuid) -> Result<Option<Vec<FollowResponse>>, reqwest::Error> {
        if let Some((access_token, validation)) = self.get_valid_access_token(&id) {
            
            let following = self.follows.get_following(&access_token, &validation.user_id, &validation.login)?;
            let online = self.follows.query_streams(&access_token, &following)?
                .into_iter()
                .map(|stream| {
                    let user = following.iter().find(|user| user.id == stream.user_id)
                        .expect(&format!("Twitch API Response to Streams contained a Stream that was not in the Request: {:?}", stream));
                    FollowResponse { profile_image_url: user.profile_image_url.clone(), offline_image_url: user.offline_image_url.clone(), stream }
                }).collect_vec();
    
            info!("Checked the {} streams {} is following. {} are online", following.len(), validation.login, online.len());
            Ok(Some(online))
        } else {
            Ok(None)
        }
    }
}