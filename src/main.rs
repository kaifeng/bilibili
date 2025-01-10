mod error;

/// Bilibili Video Dumper
/// by merging cached files to the target video.
use std::env;
use std::fmt::Display;
use std::fs;
use std::io::{Read, Seek};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use chrono::DateTime;
use clap::Parser;
use log::*;
use serde::Deserialize;

// The special file offset bilibili client cached
const SPECIAL_OFFSET: u64 = 9;
// TODO get account name from environment
const DEFAULT_SOURCE_DIR: &str = "Movies/bilibili";
const DEFAULT_TARGET_DIR: &str = "Movies/output";
const VIDEO_METADATA_FILE: &str = ".videoInfo";

// Implement a display trait
#[derive(Deserialize)]
struct VideoInfo {
    uname: String,
    title: String,
    #[serde(rename = "groupTitle")]
    group_title: String,
    pubdate: i64,
    #[serde(rename = "updateTime")]
    update_time: i64,
    #[serde(rename = "totalSize")]
    total_size: u64,
    #[serde(rename = "itemId")]
    item_id: u64,
    #[serde(rename = "coverPath")]
    cover_path: String, // should be Path later
    #[serde(rename = "groupCoverPath")]
    group_cover_path: String, // should be Path later
}

impl Display for VideoInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let dt = DateTime::from_timestamp(self.pubdate, 0).expect("invalid timestamp");
        let message = format!(
            "{} Title: {}, UP: {}, size {}, update at {}",
            self.item_id, self.title, self.uname, self.total_size, dt
        );
        f.write_str(message.as_str())
    }
}

// options
// --autoremove
// --skip-failed true
// ffmpeg path / autodetermine
// parallel processing
// Command line arguments
#[derive(Parser, Debug)]
#[command(version, long_about = None)]
#[command(about = "Bilibili Video Dumper", long_about = None)]
struct Args {
    /// Remove source files after successful conversion
    #[arg(long, default_value_t = false)]
    autoremove: bool,
    /// Do not overwrite target file if exists
    #[arg(long, default_value_t = false)]
    no_overwrite: bool,
}

fn get_metadata(path: &Path) -> Result<VideoInfo, error::Error> {
    let metafile = path.join(VIDEO_METADATA_FILE);
    let metadata_string = fs::read(&metafile)?;
    let metadata = String::from_utf8(metadata_string)?;

    Ok(serde_json::from_str(&metadata)?)
}

fn get_files_by_extension(path: &Path, extension: &str) -> Vec<PathBuf> {
    let mut filelist = Vec::new();
    let files = path.read_dir().unwrap();
    for f in files {
        let pathbuf = f.unwrap().path();
        let entry = pathbuf.as_path();
        if let Some(ext) = entry.extension() {
            if ext == extension {
                filelist.push(pathbuf);
            }
        }
    }
    debug!("get_files_by_extension {}: {:?}", extension, filelist);
    filelist
}

fn copy_to(source: &Path, target_dir: &Path) -> Result<(), error::Error> {
    let src_filename = source.file_name().ok_or(error::Error::InvalidArgument)?;
    let target_filename = target_dir.join(src_filename);
    fs::copy(source, &target_filename)?;
    Ok(())
}

fn ffmpeg_copy(input_media: &Vec<PathBuf>, output_file: &Path) -> Result<(), error::Error> {
    // ffmpeg -i source [-i source [...]] -c copy targetfile
    let mut cmd = Command::new("ffmpeg");
    for input in input_media {
        cmd.arg("-i").arg(input);
    }
    cmd.args(["-c", "copy"]).arg(output_file);
    cmd.output()?;
    Ok(())
}

fn process(path: &Path, target_path: &Path) -> Result<(), error::Error> {
    let video_info = get_metadata(path).unwrap();
    info!("Video Information: {}", video_info);

    let media = get_files_by_extension(path, "m4s");
    debug!("Media files: {:?}", media);

    let mut input_media: Vec<PathBuf> = Vec::new();
    for m in media {
        let p = m.as_path();
        let output_name = p.file_name().unwrap().to_str().unwrap();

        let mut f = fs::File::open(p).unwrap();
        let mut data: Vec<u8> = Vec::new();
        f.seek(std::io::SeekFrom::Start(SPECIAL_OFFSET)).unwrap();
        f.read_to_end(&mut data).unwrap();

        let output = target_path.join(output_name);
        fs::write(&output, data);
        input_media.push(output);
    }

    // Create target output directory
    let target_dir = if video_info.group_title != video_info.title {
        target_path.join(format!(
            "{} - {} - {}",
            video_info.uname, video_info.group_title, video_info.title
        ))
    } else {
        target_path.join(format!(
            "{} - {}",
            video_info.uname, video_info.title
        ))
    };
        
    fs::create_dir_all(&target_dir)?;

    let final_file = target_dir
        .as_path()
        .join(format!("{}.mp4", video_info.item_id));
    debug!("Final file: {:?}", final_file);

    ffmpeg_copy(&input_media, &final_file)?;

    // Remove temp media files use for ffmpeg
    for media in input_media {
        if fs::remove_file(media.as_path()).is_err() {
            error!(
                "Failed to remove temporary file {}",
                media.as_path().display()
            );
        }
    }

    // Copy photos to target directory
    info!("Copy cover art");
    copy_to(Path::new(&video_info.cover_path), &target_dir)?;
    info!("Copy group cover art");
    copy_to(Path::new(&video_info.group_cover_path), &target_dir)?;

    // Copy metadata to target directory
    // TODO Duplicated with get_metadata
    info!("Copy metadata");
    fs::copy(
        path.join(VIDEO_METADATA_FILE),
        target_dir.join("videoInfo.json"),
    );

    // Doesn't work, returns empty vector
    // let media_files: Vec<DirEntry> = files
    //     .map(|f| f.unwrap())
    //     .filter(|f| f.path().as_path().ends_with("m4s"))
    //     .collect();
    // info!("media files {:?}", media_files);

    Ok(())
}

/// Handle a directory
/// path: the directory to process
/// autoremove: if true, remove the source directory after successful processing
fn handle_dir(path: &Path, target_path: &Path, autoremove: bool) {
    let result = process(path, target_path);
    if result.is_err() {
        error!("Failed to process {}: {:?}", path.display(), result);
    } else {
        if autoremove {
            match fs::remove_dir_all(path) {
                Ok(_) => {
                    info!("Removed source directory {}", path.display());
                }
                Err(e) => error!(
                    "Failed to remove source directory {}: {}",
                    path.display(),
                    e.to_string()
                ),
            }
        }
    }
}

fn main() -> Result<(), error::Error> {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(LevelFilter::Info).init();

    let home = env::var("HOME").expect("Unable to get home directory");
    info!("Home: {}", home);

    let args = Args::parse();

    debug!("autoremove: {}", args.autoremove);
    debug!("no overwrite: {}", args.no_overwrite);

    let source_path = Path::new(&home).join(DEFAULT_SOURCE_DIR);
    info!("Source directory: {}", source_path.display());
    let subdirs = source_path
        .read_dir()
        .map_err(|_| error::Error::ReadDirectoryFailed)?;

    // Create target directory
    let target_path = Path::new(&home).join(DEFAULT_TARGET_DIR);
    info!("Target directory: {}", target_path.display());

    fs::create_dir_all(&target_path)?;

    // Iterate over subdirectories
    for dir in subdirs {
        match dir {
            Ok(entry) => {
                let p = entry.path();
                let path = p.as_path();
                if path.is_dir() {
                    handle_dir(path, &target_path, args.autoremove);
                }
            }
            Err(e) => error!("Failed to read directory: {}", e),
        }
    }

    Ok(())
}
