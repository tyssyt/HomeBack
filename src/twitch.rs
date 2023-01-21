use std::env;
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use reqwest::{Client, header};
use serde::{Serialize, Deserialize};
use log::info;
use itertools::Itertools;

pub struct Twitch {
    client: Client,
    auth_token: AuthToken,
    follow_cache: Mutex<Vec<FollowCacheEntry>>,
}

struct FollowCacheEntry {
    created_at: Instant,
    from_channel: String,
    to_users: Arc<Vec<User>>,
}

#[derive(Deserialize, Debug)]
struct AuthToken {
    access_token: String,
    expires_in: i32,
}

#[derive(Serialize, Deserialize, Debug)]
struct User {
    id: String,
    profile_image_url: String,
    offline_image_url: String,
}

#[derive(Deserialize, Debug)]
struct Follow {
    from_id: String,
    to_id: String,
}

#[derive(Deserialize, Debug)]
struct FollowData {
    data: Vec<Follow>,
    total: i32,
    pagination: Pagination,
}

#[derive(Serialize, Deserialize, Debug)]
struct Stream {
    user_id: String,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct Data<T> {
    data: Vec<T>
}

#[derive(Deserialize, Debug)]
struct Pagination {
    cursor: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct FollowResponse {
    profile_image_url: String,
    offline_image_url: String,
    #[serde(flatten)]
    stream: Stream,
}

impl Twitch {

    pub fn new() -> Twitch {
        let client_id = env::var("TWITCH_CLIENT_ID").expect("TWITCH_CLIENT_ID not set");
        let secret = env::var("TWITCH_CLIENT_SECRET").expect("TWITCH_CLIENT_SECRET not set");
        let auth_token = Twitch::make_auth_token(&client_id, &secret).unwrap();

        let mut headers = header::HeaderMap::new();
        headers.append("Client-Id", client_id.parse().unwrap());
        let client = Client::builder()
                .timeout(Duration::from_secs(2))
                .default_headers(headers)
                .build().unwrap();


        return Twitch {client, auth_token, follow_cache: Mutex::from(Vec::new())};
    }

    fn make_auth_token(id: &String, secret: &String) -> Result<AuthToken, reqwest::Error> {
        let request_url = format!("https://id.twitch.tv/oauth2/token?client_id={}&client_secret={}&grant_type=client_credentials", id, secret);
        let response: AuthToken = Client::builder().timeout(Duration::from_secs(2)).build().unwrap().post(&request_url).send()?.json()?;        
        info!("Twitch OAuth-Token Request successfull");
        return Ok(response);
    }

    //TODO Result of Option seems overkill, I doubt people care about the difference?
    pub fn get_channel_id(&self, channel: &str) -> Result<Option<String>, reqwest::Error> {
        let request_url = format!("https://api.twitch.tv/helix/users?login={}", channel);
        let mut response: Data<User> = self.client.get(&request_url)
                .bearer_auth(&self.auth_token.access_token)
                .send()?.json()?;
        if response.data.len() > 0 {
            return Ok(Some(response.data.swap_remove(0).id));
        } else {
            return Ok(None);
        }
    }

    pub fn query_users(&self, ids: &[String]) -> Result<Vec<User>, reqwest::Error> {        
        let request_url = format!("https://api.twitch.tv/helix/users?id={}", ids.join("&id="));
        let response: Data<User> = self.client.get(&request_url)
                .bearer_auth(&self.auth_token.access_token)
                .send()?.json()?;
        return Ok(response.data);
    }

    pub fn get_following(&self, from_channel: &str) -> Result<Arc<Vec<User>>, reqwest::Error> {
        {   //clean cache
            let now = Instant::now();
            let mut cache = self.follow_cache.lock().unwrap();        
            cache.retain(|entry| entry.created_at.elapsed().as_secs() < 24*60*60);

            if let Some(cache_entry) = cache.iter().find(|entry| entry.from_channel == from_channel) {            
                return Ok(cache_entry.to_users.clone());
            }
        }

        // find channel id
        let id = self.get_channel_id(&from_channel)?;
        if id.is_none() {
            return Ok(Arc::new(Vec::new()));
        }

        // find the channels the user is following
        let following = self.query_following(&id.unwrap())?;
        info!("Loaded & Cached the {} streams {} is following", following.len(), from_channel);

        // lookup the user data of the followed channels
        let users = Arc::new({
            let mut users = Vec::new();
            for chunk in following.chunks(100) {
                users.append(&mut self.query_users(chunk)?);
            }
            users
        });

        {
            let mut cache = self.follow_cache.lock().unwrap(); 
            cache.push(FollowCacheEntry{created_at: Instant::now(), from_channel: from_channel.to_owned(), to_users: users.clone()});
        }

        return Ok(users);
    }


    fn query_following(&self, from_id: &String) -> Result<Vec<String>, reqwest::Error> {
        let request_url = format!("https://api.twitch.tv/helix/users/follows?from_id={}&first=100", from_id);

        let mut response: FollowData = self.client.get(&request_url)
                .bearer_auth(&self.auth_token.access_token)
                .send()?.json()?;
        let mut following: Vec<String> = response.data.into_iter().map(|follow| follow.to_id).collect();
        
        while response.pagination.cursor.is_some() {
            let request_url_after = format!("https://api.twitch.tv/helix/users/follows?from_id={}&first=100&after={}", from_id, response.pagination.cursor.unwrap());
            response = self.client.get(&request_url_after)
                    .bearer_auth(&self.auth_token.access_token)
                    .send()?.json()?;
            
            following.extend(response.data.into_iter().map(|follow| follow.to_id));
        }

        following.push(from_id.clone()); // also show the user if he is online
        return Ok(following);
    }

    pub fn get_online_following(&self, from_channel: String) -> Result<Vec<FollowResponse>, reqwest::Error> {
        let following = self.get_following(&from_channel)?;

        let mut online: Vec<FollowResponse> = Vec::new();
        for chunk in following.chunks(100) {
            let request_url = format!("https://api.twitch.tv/helix/streams?first=100&user_id={}", chunk.iter().map(|user| &user.id).join("&user_id="));
            let twitch_response: Data<Stream> = self.client.get(&request_url)
                    .bearer_auth(&self.auth_token.access_token)
                    .send()?.json()?;
            
            for stream in twitch_response.data {
                let user = chunk.iter().find(|user| user.id == stream.user_id)
                    .expect(&format!("Twitch API Response to Streams contained a Stream that was not in the Request: {:?}", stream));
                online.push(FollowResponse { profile_image_url: user.profile_image_url.clone(), offline_image_url: user.offline_image_url.clone(), stream });
            }
        }

        info!("Checked the {} streams {} is following. {} are online", following.len(), from_channel, online.len());
        return Ok(online);
    }

}