use std::env;
use log::info;
use std::time::{Duration, Instant};
use reqwest::blocking::Client;
use std::sync::{Arc, Mutex};

// TODO switch to non-blocking reqwest
// TODO more logging

pub struct DvbC {
    client: Client,
    url_hd: String,
    url_sd: String,
    url_radio: String,
    channels: Mutex<Option<Arc<Channels>>>,
}

pub struct Channels {
    pub tv:    Vec<Channel>,
    pub radio: Vec<Channel>,
    fetched_at: Instant
}

#[derive(Clone, PartialEq)]
pub struct Channel {
    pub name: String,
    pub url: String,
}

fn needs_update(channels: &Option<Arc<Channels>>) -> bool {
    channels.is_none() || Instant::now().duration_since(channels.as_ref().unwrap().fetched_at).as_secs() > 60*60
}

impl DvbC {

    pub fn new() -> DvbC {
        let router_url = env::var("ROUTER_URL").expect("ROUTER_URL not set");
        return DvbC {
            client:    Client::builder().timeout(Duration::from_secs(2)).build().unwrap(),
            url_hd:    format!("{}{}", router_url, "/dvb/m3u/tvhd.m3u"),
            url_sd:    format!("{}{}", router_url, "/dvb/m3u/tvsd.m3u"),
            url_radio: format!("{}{}", router_url, "/dvb/m3u/radio.m3u"),
            channels:  Mutex::new(None),
        };
    }

    pub fn get_channels(&self) -> Option<Arc<Channels>> {
        let mut lock = self.channels.lock().unwrap();
        if needs_update(&*lock) {
            *lock = self.fetch_all_channels().ok().map(|c| Arc::new(c));
        }
        return lock.clone();
    }

    fn fetch_all_channels(&self) -> Result<Channels, reqwest::Error> {
        let mut tv =   self.fetch_category(&self.url_hd)?;
        tv.append(&mut self.fetch_category(&self.url_sd)?);
        let radio  =   self.fetch_category(&self.url_radio)?;
        info!("Loaded DvbC: {} TV & {} Radio Channels", tv.len(), radio.len());
        Ok(Channels {
            tv,
            radio: radio,
            fetched_at: Instant::now()
        })
    }

    fn fetch_category(&self, url: &str) -> Result<Vec<Channel>, reqwest::Error> {
        let text = self.client.get(url).send()?.text()?;
        let mut lines = text.lines().skip(1);
        
        let mut channels = Vec::new();
        loop {
            if let (Some(first), Some(_second), Some(third)) = (lines.next(), lines.next(), lines.next()) {
                channels.push(Channel {name: String::from(&first[10..]), url: String::from(third)})
            } else {
                break;
            }
        }
        Ok(channels)
    }
}
