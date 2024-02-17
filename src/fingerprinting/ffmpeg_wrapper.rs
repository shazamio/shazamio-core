use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Write};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use rodio::Decoder;
use std::io::Cursor;
use std::process::Command;
use tempfile::Builder;

/// This function used to decode a file with FFMpeg, if it is installed on
/// the system, in the case where Rodio can't decode the concerned format
/// (for example with .WMA, .M4A, etc.).

pub fn decode_with_ffmpeg(file_path: &str) -> Option<Decoder<BufReader<File>>> {
    // Find the path for FFMpeg, in the case where it is installed

    let mut possible_ffmpeg_paths: Vec<&str> = vec!["ffmpeg", "ffmpeg.exe"];

    let mut current_dir_ffmpeg_path =
        std::env::current_exe().expect("failed current_dir_ffmpeg_path");
    current_dir_ffmpeg_path.pop();
    current_dir_ffmpeg_path.push("ffmpeg.exe");
    possible_ffmpeg_paths.push(current_dir_ffmpeg_path.to_str().unwrap());
    let mut actual_ffmpeg_path: Option<&str> = None;

    for possible_path in possible_ffmpeg_paths {
        // Use .output() to execute the subprocess testing for FFMpeg
        // presence and correct execution, so that it does not pollute
        // the standard or error output in any way

        let mut command = Command::new(possible_path);
        #[cfg(windows)]
        let command = command.creation_flags(0x08000000);
        let command = command.arg("-version");

        if let Ok(process) = command.output() {
            if process.status.success() {
                actual_ffmpeg_path = Some(possible_path);
                break;
            }
        }
    }

    // If FFMpeg is available, use it to convert the input file
    // from whichever format to a .WAV (because Rodio has its
    // decoding support limited to .WAV, .FLAC, .OGG, .MP3, which
    // makes that .MP4/.AAC, .OPUS or .WMA are not supported, and
    // Rodio's minimp3 .MP3 decoder seems to crash on Windows anyway)
    if let Some(ffmpeg_path) = actual_ffmpeg_path {
        // Create a sink file for FFMpeg

        let sink_file = Builder::new().suffix(".wav").tempfile().unwrap();

        let sink_file_path = sink_file.into_temp_path();
        // Try to convert the input video or audio file to a standard
        // .WAV s16le PCM file using FFMpeg, and pass it to Rodio
        // later in the case where it succeeded

        let mut command = Command::new(ffmpeg_path);

        #[cfg(windows)]
        let command = command.creation_flags(0x08000000);

        let command = command.args(["-y", "-i", file_path, sink_file_path.to_str().unwrap()]);

        // Set "CREATE_NO_WINDOW" on Windows, see
        // https://stackoverflow.com/a/60958956/662399

        if let Ok(process) = command.output() {
            if process.status.success() {
                let res = Decoder::new(BufReader::new(
                    File::open(sink_file_path.to_str().unwrap()).unwrap(),
                ))
                .expect("failed to decode with ffmpeg");
                return Some(res);
            }
        } else {
            return None;
        }
    }
    None
}

pub fn decode_with_ffmpeg_from_bytes(
    bytes: &[u8],
) -> Result<Decoder<Cursor<Vec<u8>>>, Box<dyn Error>> {
    // Create a temporary file for the input
    let mut temp_file = Builder::new().suffix(".tmp").tempfile()?;
    temp_file.write_all(bytes)?;
    let file_path = temp_file.path().to_str().unwrap().to_string();

    // Find the FFmpeg path
    let mut possible_ffmpeg_paths = vec!["ffmpeg", "ffmpeg.exe"];
    let mut current_dir_ffmpeg_path =
        std::env::current_exe().expect("failed current_dir_ffmpeg_path");
    current_dir_ffmpeg_path.pop();
    current_dir_ffmpeg_path.push("ffmpeg.exe");
    possible_ffmpeg_paths.push(current_dir_ffmpeg_path.to_str().unwrap());

    let mut actual_ffmpeg_path = None;
    for possible_path in &possible_ffmpeg_paths {
        let mut command = Command::new(possible_path);
        #[cfg(windows)]
        let command = command.creation_flags(0x08000000);
        let command = command.arg("-version");

        if let Ok(process) = command.output() {
            if process.status.success() {
                actual_ffmpeg_path = Some(*possible_path);
                break;
            }
        }
    }

    if let Some(ffmpeg_path) = actual_ffmpeg_path {
        // Create a temporary file for the output
        let sink_file = Builder::new().suffix(".wav").tempfile()?;
        let sink_file_path = sink_file.path().to_str().unwrap().to_string();

        // Convert to WAV format
        let mut command = Command::new(ffmpeg_path);
        #[cfg(windows)]
        let command = command.creation_flags(0x08000000);
        let command = command.args(["-y", "-i", &file_path, &sink_file_path]);

        if let Ok(process) = command.output() {
            if process.status.success() {
                // Read the converted file into a Vec<u8>
                let converted_bytes = std::fs::read(sink_file_path)?;

                // Create a Cursor around the converted bytes and create a Decoder
                let cursor = Cursor::new(converted_bytes);
                let decoder = Decoder::new(cursor)?;

                return Ok(decoder);
            }
        }
    }
    Err("FFmpeg not found or failed to convert audio".into())
}
