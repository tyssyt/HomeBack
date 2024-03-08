use super::{Data, PagedData};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use reqwest::blocking::Client;
use reqwest::header;
use serde::{Serialize, Deserialize};
use itertools::Itertools;

use log::info;

// TODO switch to non-blocking reqwest

pub struct TwitchFollows {
    client: Client,
    follow_cache: Mutex<Vec<FollowCacheEntry>>,
}

struct FollowCacheEntry {
    created_at: Instant,
    from: String,
    to: Arc<Vec<User>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Stream {
    pub user_id: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct User {
    pub id: String,
    pub profile_image_url: String,
    pub offline_image_url: String,
}

#[derive(Deserialize, Debug)]
struct Follow {
    broadcaster_id: String,
}

impl TwitchFollows {

    pub fn new(client_id: &str) -> Self {        
        let mut headers = header::HeaderMap::new();
        headers.append("Client-Id", client_id.parse().unwrap());
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .default_headers(headers)
            .build().unwrap();

        Self { client, follow_cache: Mutex::from(Vec::new()) }
    }

    fn get_cached(&self, user_id: &str) -> Option<Arc<Vec<User>>> {
        //clean cache
        let mut cache = self.follow_cache.lock().unwrap();        
        cache.retain(|entry| entry.created_at.elapsed().as_secs() < 24*60*60);

        cache.iter()
            .find(|entry| entry.from == user_id)
            .map(|entry| entry.to.clone())
    }

    fn cache(&self, user_id: &str, users: Vec<User>) -> Arc<Vec<User>> {        
        let arc = Arc::new(users);
        let mut cache = self.follow_cache.lock().unwrap(); 
        cache.push( FollowCacheEntry{created_at: Instant::now(), from: user_id.to_owned(), to: arc.clone()} );
        arc
    }    

    pub fn get_following(&self, access_token: &str, user_id: &str, user_name: &str) -> Result<Arc<Vec<User>>, reqwest::Error> {
        if let Some(cached) = self.get_cached(user_id) {        
            return Ok(cached);
        }

        let users = self.cache(user_id, self.query_users(access_token, self.query_following(access_token, &user_id)?)?);
        info!("Loaded & Cached the {} streams {} is following", users.len(), user_name);
        return Ok(users);
    }

    fn query_following(&self, access_token: &str, from_id: &str) -> Result<Vec<String>, reqwest::Error> {
        let url = format!("https://api.twitch.tv/helix/channels/followed?user_id={}&first=100", from_id);
        let mut response: PagedData<Follow>= self.client.get(&url)
            .bearer_auth(access_token)
            .send()?.error_for_status()?.json()?;
        let mut following: Vec<String> = response.data.into_iter().map(|follow| follow.broadcaster_id).collect();
        
        while response.pagination.cursor.is_some() {
            let url_after = format!("https://api.twitch.tv/helix/channels/followed?user_id={}&first=100&after={}", from_id, response.pagination.cursor.unwrap());
            response = self.client.get(&url_after)
                .bearer_auth(access_token)
                .send()?.error_for_status()?.json()?;            
            following.extend(response.data.into_iter().map(|follow| follow.broadcaster_id));
        }

        following.push(from_id.to_owned()); // also show the user if he is online
        return Ok(following);
    }

    fn query_users(&self, access_token: &str, ids: Vec<String>) -> Result<Vec<User>, reqwest::Error> {
        let mut users = Vec::new();
        for chunk in ids.chunks(100) {
            let url = format!("https://api.twitch.tv/helix/users?id={}", chunk.join("&id="));
            let mut response: Data<User> = self.client.get(&url)
                .bearer_auth(access_token)
                .send()?.error_for_status()?.json()?;
            users.append(&mut response.data);
        }
        Ok(users)
    }

    pub fn query_streams(&self, access_token: &str, users: &Vec<User>) -> Result<Vec<Stream>, reqwest::Error>  {
        let mut streams: Vec<Stream> = Vec::new();
        for chunk in users.chunks(100) {
            let url = format!("https://api.twitch.tv/helix/streams?first=100&user_id={}", chunk.iter().map(|user| &user.id).join("&user_id="));
            let mut response: Data<Stream> = self.client.get(&url)
                .bearer_auth(&access_token)
                .send()?.error_for_status()?.json()?;
            streams.append(&mut response.data);
        }
        Ok(streams)
    }
    
}