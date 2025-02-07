mod error;

/// Bilibili Video converter
/// by merging cached files to the target video.
use std::env;
use std::fmt::Display;
use std::fs;
use std::io::{Read, Seek};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use chrono::DateTime;
use clap::{Parser, Subcommand};
use log::*;
use serde::Deserialize;

// The special file offset bilibili client cached
const SPECIAL_OFFSET: u64 = 9;

const DEFAULT_SOURCE_DIR: &str = "Movies/bilibili";
const DEFAULT_TARGET_DIR: &str = "Movies/output";
const VIDEO_METADATA_FILE: &str = ".videoInfo";

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
    info!("Video: {}", video_info);

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
    debug!("Copy cover art");
    copy_to(Path::new(&video_info.cover_path), &target_dir)?;
    debug!("Copy group cover art");
    copy_to(Path::new(&video_info.group_cover_path), &target_dir)?;

    // Copy metadata to target directory
    debug!("Copy metadata");
    fs::copy(
        path.join(VIDEO_METADATA_FILE),
        target_dir.join("videoInfo.json"),
    );

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

fn get_video_list(path: &Path) -> Result<Vec<VideoInfo>, error::Error> {
    
    let mut video_list = Vec::<VideoInfo>::new();

    let subdirs = path
    .read_dir()
    .map_err(|_| error::Error::ReadDirectoryFailed)?;

    for dir in subdirs {
        match dir {
            Ok(entry) => {
                let p = entry.path();
                let path = p.as_path();
                if path.is_dir() {
                    let video_info = get_metadata(path)?;
                    video_list.push(video_info);
                }
            }
            Err(e) => error!("Failed to read directory: {}", e),
        }
    }

    Ok(video_list)

}

#[derive(Subcommand, Debug)]
enum Commands {
    List,
    Convert {
        item: Option<String>,
    },
    Clean {
        item: Option<String>,
    },
}

// Command line arguments
// --autoremove
// --skip-failed true
#[derive(Parser, Debug)]
#[command(version, long_about = None)]
#[command(about = "Bilibili Video Converter", long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
    /// Enable debug output
    #[arg(short, default_value_t = false)]
    verbose: bool,
    /// Remove source files after successful conversion
    #[arg(long, default_value_t = false)]
    autoremove: bool,
    /// Do not overwrite target file if exists
    #[arg(long, default_value_t = false)]
    no_overwrite: bool,
}

fn check_environment() -> Result<(), error::Error> {

    // Check if ffmpeg is available
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("ffmpeg is not installed or not found in PATH");
        return Err(error::Error::CommandNotFound);
    }
    Ok(())
}

fn main() -> Result<(), error::Error> {

    let args = Args::parse();

    let log_level = match args.verbose {
        true => LevelFilter::Debug,
        false =>  LevelFilter::Info,
    };

    let mut builder = env_logger::Builder::new();
    builder.filter_level(log_level).init();

    let home = env::var("HOME").expect("Unable to get home directory");
    
    info!("Home: {}", home);
    debug!("autoremove: {}", args.autoremove);
    debug!("no overwrite: {}", args.no_overwrite);
    
    let source_path = Path::new(&home).join(DEFAULT_SOURCE_DIR);
    debug!("Source directory: {}", source_path.display());
    let subdirs = source_path
        .read_dir()
        .map_err(|_| error::Error::ReadDirectoryFailed)?;

    let specified_item = match args.command {
        Commands::List => {
            let videos = get_video_list(&source_path)?;
            for video in videos {
                println!("{}", video);
            }
            return Ok(());
        },
        Commands::Convert { item } => {
            item
        },
        // this is danger and should need a confirmation
        Commands::Clean { item } => {
            if let Some(item) = item {
                let item_path = source_path.join(item);
                warn!("Removing directory {:?}", item_path);
                fs::remove_dir_all(item_path)?;
            } else {
                for dir in subdirs {
                    match dir {
                        Ok(entry) => {
                            let p = entry.path();
                            let path = p.as_path();
                            if path.is_dir() {
                                warn!("Removing directory {:?}", entry);
                                fs::remove_dir_all(path)?;
                            }
                        }
                        Err(e) => error!("Failed to read directory: {}", e),
                    }
                }
            }
            return Ok(());
        }
    };

    check_environment()?;

    // Create target directory before processing
    let target_path = Path::new(&home).join(DEFAULT_TARGET_DIR);
    debug!("Target directory: {}", target_path.display());
    fs::create_dir_all(&target_path)?;

    // Handle the item if specified, otherwise process all by iterating over subdirectories
    // TODO Make video processing in a uniform way by passing items to process
    if let Some(item) = specified_item {
        let item_path = source_path.join(item);
        handle_dir(&item_path, &target_path, args.autoremove);
    } else {
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
    }

    Ok(())
}
