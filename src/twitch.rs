use std::env;
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use reqwest::{Client, header};
use serde::Deserialize;

pub struct Twitch {
    client: Client,
    auth_token: AuthToken,

    follow_cache: Mutex<Vec<(Instant, String, Arc<Vec<String>>)>>,
}

#[derive(Deserialize, Debug)]
struct AuthToken {
    access_token: String,
    expires_in: i32,
}

#[derive(Deserialize, Debug)]
struct User {
    id: String,
//    login: String,
//    display_name: String,
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

#[derive(Deserialize, Debug)]
struct Data<T> {
    data: Vec<T>
}

#[derive(Deserialize, Debug)]
struct Pagination {
    cursor: Option<String>,
}

//TODO also load user profile image url
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
        return Ok(response);
    }

    //TODO Result of Option seems overkill, I doubt people care about the difference?
    pub fn get_channel_id(&self, channel: &String) -> Result<Option<String>, reqwest::Error> {
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

    pub fn get_following(&self, from_channel: String) -> Result<Arc<Vec<String>>, reqwest::Error> {
        {   //clean cache
            let now = Instant::now();
            let mut cache = self.follow_cache.lock().unwrap();        
            cache.retain(|(ts, _, _)| now.duration_since(*ts).as_secs() < 24*60*60); //TODO no idea what the * does here, that really should not work...

            if let Some(cache_entry) = cache.iter().find(|(_, channel, _)| channel == &from_channel) {            
                return Ok(cache_entry.2.clone());
            }
        }

        let id = self.get_channel_id(&from_channel)?;
        if id.is_none() {
            return Ok(Arc::new(Vec::new()));
        }
        let following = self.query_following(&id.unwrap())?;      
        let ret = Arc::new(following);

        let mut cache = self.follow_cache.lock().unwrap(); 
        cache.push((Instant::now(), from_channel, ret.clone()));
        return Ok(ret);
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

    pub fn get_online_following(&self, from_channel: String) -> Result<Vec<serde_json::Value>, reqwest::Error> {
        let following = self.get_following(from_channel)?;
        let mut online: Vec<serde_json::Value> = Vec::new();

        for chunk in following.chunks(100) {
            let request_url = format!("https://api.twitch.tv/helix/streams?first=100&user_id={}", chunk.join("&user_id="));
            let mut response: Data<serde_json::Value> = self.client.get(&request_url)
                    .bearer_auth(&self.auth_token.access_token)
                    .send()?.json()?;
            online.append(&mut response.data);
        }

        return Ok(online);
    }

}