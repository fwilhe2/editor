// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The command line, parsed with no window in sight.
//!
//! Every GUI shell takes the same four things — a file, and the three that make a frame
//! assertable: `--render-to` draws one frame into an image and exits, `--size` says how big,
//! `--dark` which palette. That is how a custom-drawn widget gets an output a test (and CI) can
//! compare: two renders of one document must be byte-identical, and a refactor of the drawing
//! code is proved to be one when the image does not change (`doc/architecture.md` §5).

use std::path::PathBuf;

pub const USAGE: &str = "usage: quill-gtk [FILE] [--render-to PNG] [--size WxH] [--dark]

  FILE             the document; one that does not exist yet is created on the first save
  --render-to PNG  draw one frame of FILE into PNG and exit, with no display
  --size WxH       the frame's size in pixels (default 800x600)
  --dark           draw it with the dark palette
";

#[derive(Debug, PartialEq)]
pub struct Args {
    pub file: Option<PathBuf>,
    pub render_to: Option<PathBuf>,
    pub size: (i32, i32),
    pub dark: bool,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        file: None,
        render_to: None,
        size: (800, 600),
        dark: false,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--render-to" => {
                let path = args.next().ok_or("--render-to needs a path")?;
                parsed.render_to = Some(PathBuf::from(path));
            }
            "--size" => {
                let size = args.next().ok_or("--size needs WxH")?;
                parsed.size = size
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    .filter(|&(w, h): &(i32, i32)| w > 0 && h > 0)
                    .ok_or_else(|| format!("--size {size:?} is not WxH"))?;
            }
            "--dark" => parsed.dark = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            file if parsed.file.is_none() => parsed.file = Some(PathBuf::from(file)),
            extra => return Err(format!("one file at a time ({extra:?} is a second)")),
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn a_file_and_a_frame() {
        let parsed = args(&[
            "a.txt",
            "--render-to",
            "f.png",
            "--size",
            "320x200",
            "--dark",
        ]);
        assert_eq!(
            parsed.unwrap(),
            Args {
                file: Some("a.txt".into()),
                render_to: Some("f.png".into()),
                size: (320, 200),
                dark: true,
            }
        );
    }

    #[test]
    fn mistakes_are_refused() {
        assert!(args(&["--size", "wide"]).is_err());
        assert!(args(&["--frobnicate"]).is_err());
        assert!(args(&["a", "b"]).is_err());
        assert!(args(&["--render-to"]).is_err());
    }
}
