use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf, Component};
use std::error::Error;
use reqwest::Client;
use threadpool::ThreadPool;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use serde::Serialize;

lazy_static! {
    static ref SCAN_FOLDER :     PathBuf = PathBuf::from(env::var("SCAN_FOLDER").expect("SCAN_FOLDER not set"));
    static ref DOWNLOAD_FOLDER : PathBuf = PathBuf::from(env::var("DOWNLOAD_FOLDER").expect("DOWNLOAD_FOLDER not set"));
}

fn sanitize_path(path: &str) -> PathBuf {
    Path::new(path).components().filter(|c| c != &Component::ParentDir).collect()
}

pub fn read_scan_folder() -> io::Result<Vec<String>> { 
    Ok(fs::read_dir(&*SCAN_FOLDER)?
        .filter_map(|file| file.ok())
        .filter(|file| file.file_type().map_or(false, |f_type| f_type.is_file()))
        .map(|file| file.file_name().into_string().unwrap())
        .collect())
}

pub fn read_scan_file(file: String) -> io::Result<Vec<String>> {
    let mut content: &str = &fs::read_to_string(SCAN_FOLDER.join(sanitize_path(&file)))?;

    let mut links = Vec::new();
    loop {
        if let Some(start) = content.find("https://sinbad.hi10") {
            content = &content[start..];
            if let Some(end) = content.find("\">") {
                let link = String::from(&content[..end]);
                links.push(link);            
            }
            content = &content[1..];
        } else {
            break;
        }
    }

    links.sort();
    links.dedup();

    return Ok(links);
}

pub fn delete_scan_file(file: String) -> io::Result<()> {
    fs::remove_file(SCAN_FOLDER.join(sanitize_path(&file)))
}

pub fn find_in_download_folder(file: &str) -> bool { 
    DOWNLOAD_FOLDER.join(sanitize_path(&file)).exists()
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
    Finished,
    Error
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

        self.threadpool.lock()?.execute(move || DownloadManager::dl_wrap(&self.client, download, query));
        Ok(raw_download)
    }

    fn dl_wrap(client: &Client, download: Arc<Mutex<Download>>, query: Option<String>) {
        if let Err(error) = DownloadManager::dl(client, &download, query) {
            let mut dl = download.lock().unwrap();
            dl.status = Status::Error;
            println!("Error during Download: {}", error);
        }
    }

    fn dl(client: &Client, download: &Arc<Mutex<Download>>, query: Option<String>) -> Result<(), Box<dyn Error>> {
        let (mut response, mut file) = {
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

            (response, file)
        }; //unlock mutex

        //this locks the thread until download is completed
        response.copy_to(&mut file)?;

        let mut dl = download.lock().unwrap();
        dl.status = Status::Finished;
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

    pub fn delete_download(&self, uuid: Uuid) {
        let mut downloads = self.downloads.lock().unwrap();
        if let Some(pos) = downloads.iter().position(|dl| dl.lock().unwrap().uuid == uuid) {
            let removed = downloads.remove(pos);
            let rm = removed.lock().unwrap();
            if rm.status == Status::Running {
                //TODO cancel Download
            }
        }
    }
}

impl Download {
    pub fn update_size(&mut self) {
        if let Ok(metadata) = DOWNLOAD_FOLDER.join(sanitize_path(&self.path)).metadata() {
            self.current_size = metadata.len();
        }
    }
}


