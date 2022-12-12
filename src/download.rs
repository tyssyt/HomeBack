use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::error::Error;
use reqwest::Client;
use threadpool::ThreadPool;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use serde::Serialize;
use super::files::sanitize_path;
use lazy_static::lazy_static;
use regex::Regex;
use log::error;

lazy_static! {
    static ref SCAN_FOLDER :     PathBuf = PathBuf::from(env::var("SCAN_FOLDER").expect("SCAN_FOLDER not set"));
    static ref DOWNLOAD_FOLDER : PathBuf = PathBuf::from(env::var("DOWNLOAD_FOLDER").expect("DOWNLOAD_FOLDER not set"));
}

pub fn read_scan_folder() -> io::Result<Vec<String>> { 
    Ok(fs::read_dir(&*SCAN_FOLDER)?
        .filter_map(|file| file.ok())
        .filter(|file| file.file_type().map_or(false, |f_type| f_type.is_file()))
        .map(|file| file.file_name().into_string().unwrap())
        .collect())
}

pub fn read_scan_file(file: String) -> io::Result<Vec<String>> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r#"https://[A-Za-z0-9]+?\.hi10an[^>";]*"#).unwrap();
    }

    let content: &str = &fs::read_to_string(SCAN_FOLDER.join(sanitize_path(&file)))?;
    
    let mut links = RE.find_iter(content)
        .map(|m| m.as_str().to_string() )
        .filter(|link| !link.starts_with("https://stream."))
        .collect::<Vec<String>>();

    links.sort();
    links.dedup();

    return Ok(links);
}

pub fn read_downloads_subfolder(subfolder: String) -> io::Result<Vec<String>> {
    Ok(fs::read_dir(DOWNLOAD_FOLDER.join(sanitize_path(&subfolder)))?
        .filter_map(|file| file.ok())
        .filter(|file| file.file_type().map_or(false, |f_type| f_type.is_file()))
        .map(|file| file.file_name().into_string().unwrap())
        .collect())
}

pub struct DownloadManager {
    client: Client,
    threadpool: Mutex<ThreadPool>, //Mutex because: https://github.com/rust-threadpool/rust-threadpool/issues/96
    downloads: Mutex<Vec<Arc<Mutex<Download>>>>, // :>>
}

#[derive(Serialize, Clone, PartialEq)]
pub enum Status {
    Created,
    Running,
    Error,
}

#[derive(Serialize, Clone)]
pub struct Download {
    status: Status,
    pub uuid: Uuid,
    url: String,
    path: String,
    current_size: u64,
    size: Option<u64>,
}

impl DownloadManager {
    
    pub fn new() -> DownloadManager {
        let client = Client::builder().build().unwrap();
        return DownloadManager {client, threadpool: Mutex::new(ThreadPool::new(6)), downloads: Mutex::new(Vec::new())};
    }

    pub fn trigger_download(&'static self, url: String, path: String, query: Option<String>) -> Result<Download, Box<dyn Error>> {
        let uuid = Uuid::new_v4();
        let raw_download = Download{status: Status::Created, uuid, url, path, current_size: 0, size: None};
        let download = Arc::new(Mutex::new(raw_download.clone()));
        self.downloads.lock()?.push(download.clone());

        self.threadpool.lock()?.execute(move || self.dl_wrap(&self.client, download, query));
        Ok(raw_download)
    }

    fn dl_wrap(&self, client: &Client, download: Arc<Mutex<Download>>, query: Option<String>) {
        if let Err(error) = self.dl(client, &download, query) {
            let mut dl = download.lock().unwrap();
            dl.status = Status::Error;
            error!("Error during Download: {}", error);
        }
    }

    fn dl(&self, client: &Client, download: &Arc<Mutex<Download>>, query: Option<String>) -> Result<(), Box<dyn Error>> {
        let (mut response, mut file, uuid) = {
            let mut dl = download.lock().unwrap();

            //prepare query
            let response = {
                match query {
                    Some(q) => {let target = format!("{}?{}", &dl.url, q); client.get(&target)},
                    None    => client.get(&dl.url)
                }
            }.send()?;

            //prepare file
            let path = DOWNLOAD_FOLDER.join(sanitize_path(&dl.path));
            fs::create_dir_all(path.parent().unwrap())?;
            let file = fs::File::create(path)?;

            //get file size
            dl.size = response.content_length();
            dl.status = Status::Running;

            (response, file, dl.uuid)
        }; //unlock mutex

        //this locks the thread until download is completed
        response.copy_to(&mut file)?;

        //download finished, remove
        self.remove_download(uuid);
        Ok(())
    }

    pub fn get_download(&self, uuid: Uuid) -> Option<Arc<Mutex<Download>>> {
        let dls = { //lock scope
            self.downloads.lock().unwrap()
                .iter()
                .find(|dl| dl.lock().unwrap().uuid == uuid)
                .map(|dl| dl.clone())
        };
        for dl in &dls {
            dl.lock().unwrap().update_size();
        }
        dls
    }

    pub fn get_downloads(&self) -> Vec<Arc<Mutex<Download>>> {
        let dls = { //lock scope
            self.downloads.lock().unwrap().clone()
        };
        for dl in &dls {
            dl.lock().unwrap().update_size();
        }
        dls
    }

    pub fn cancel_download(&self, uuid: Uuid) {
        if let Some(download) = self.remove_download(uuid) {
            /*
            let dl = download.lock().unwrap();
            match dl.status {
                Status::Created => // remove from queue
                Status::Running => // cancel download
                Status::Error => // just remove from list, maybe remove the file as well?
            }
            */
        }
    }

    fn remove_download(&self, uuid: Uuid) -> Option<Arc<Mutex<Download>>> {
        let mut downloads = self.downloads.lock().unwrap();
        downloads.iter()
            .position(|dl| dl.lock().unwrap().uuid == uuid)
            .map(|pos| downloads.remove(pos))
    }

}

impl Download {
    pub fn update_size(&mut self) {
        if let Ok(metadata) = DOWNLOAD_FOLDER.join(sanitize_path(&self.path)).metadata() {
            self.current_size = metadata.len();
        }
    }
}


