use std::time::Duration;
use reqwest::blocking::Client;
use reqwest:: StatusCode;
use serde::Deserialize;

// TODO switch to non-blocking reqwest

pub struct TwitchAuthClient {
    client: Client,
    client_id: String,
    client_secret: String,
}

#[derive(Deserialize, Debug)]
pub struct AuthorizationRequest {
    pub device_code: String,
    pub expires_in: u64,
    pub interval: i32,
    pub user_code: String,
    pub verification_uri: String,    
}

#[derive(Deserialize, Debug)]
pub struct Authorization {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Deserialize, Debug)]
pub struct Validation {
    pub user_id: String,
    pub login: String,
}

#[derive(Deserialize, Debug)]
struct BadRequestBody {
    message: String,
}

impl TwitchAuthClient {
    pub fn new(client_id: String, client_secret: String) -> Self {
        let client = Client::builder().timeout(Duration::from_secs(1)).build().unwrap();
        return Self{client, client_id, client_secret};
    }

    pub fn create_authorization_request(&self) -> Result<AuthorizationRequest, reqwest::Error> {
        let url = format!("https://id.twitch.tv/oauth2/device?client_id={}&scopes=user:read:follows", self.client_id);
        self.client.post(url).send()?.error_for_status()?.json()
    }

    pub fn activate_authorization_request(&self, device_code: &str) -> Result<Option<Authorization>, reqwest::Error> {
        let url = format!("https://id.twitch.tv/oauth2/token?client_id={}&device_code={}&grant_type=urn:ietf:params:oauth:grant-type:device_code", self.client_id, device_code);
        let response = self.client.post(url).send()?;

        match response.error_for_status_ref() {
            Ok(_) =>  Ok(Some(response.json()?)),
            Err(error) => {
                if response.status() == StatusCode::BAD_REQUEST && response.json::<BadRequestBody>()?.message == "authorization_pending" {
                    Ok(None)
                } else {
                    Err(error)
                }
            }
        }
    }

    pub fn validate_authorization(&self, access_token: &str) -> Result<Option<Validation>, reqwest::Error> {
        let response = self.client.get("https://id.twitch.tv/oauth2/validate").bearer_auth(&access_token).send()?;
        if response.status() == StatusCode::UNAUTHORIZED {
            Ok(None)
        } else {
            Ok(Some(response.error_for_status()?.json()?))
        }
    }

    pub fn refresh_authorization(&self, refresh_token: &str) -> Result<Authorization, reqwest::Error> {
        let url = format!("https://id.twitch.tv/oauth2/token?grant_type=refresh_token&refresh_token={}&client_id={}&client_secret={}", refresh_token, self.client_id, self.client_secret);
        self.client.post(url).send()?.error_for_status()?.json()
    }

}