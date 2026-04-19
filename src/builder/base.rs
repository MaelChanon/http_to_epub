use std::io::Cursor;
use image::ImageFormat;
use bytes::Bytes;
use crate::provider::base::{TChapter, TMangaInfo};

pub struct BuildParams {
    pub height: u32,
    pub width: u32,
    pub cover: Bytes,
    pub chapters: Vec<Box<dyn TChapter + Send + Sync>>,
    pub manga_info: Box<dyn TMangaInfo>,
    pub creator: String,
    pub lang: String,
    pub split_double_page: bool
}

pub enum FileMod {
    CBZ(BuildParams),
    EPUB(BuildParams),
}

pub(crate) fn encode_image(bytes: Vec<u8>, target_width: u32, target_height: u32, split_double_page: bool) -> Result<Vec<(Vec<u8>, u32, u32)>, Box<dyn std::error::Error + Send + Sync>> {
    let img = image::load_from_memory(&bytes)?;
    let (w, h) = (img.width(), img.height());
    if split_double_page && w > h {
        let half = w / 2;
        let left = img.crop_imm(0, 0, half, h);
        let right = img.crop_imm(half, 0, w - half, h);

        let encode_half = |half_img: image::DynamicImage| -> Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error + Send + Sync>> {
            let resized = half_img.thumbnail(target_width, target_height);
            let (rw, rh) = (resized.width(), resized.height());
            let mut output = Vec::new();
            resized.write_to(&mut Cursor::new(&mut output), ImageFormat::Jpeg)?;
            Ok((output, rw, rh))
        };

        // right page first (manga reading order)
        let (right_result, left_result) = rayon::join(
            || encode_half(right),
            || encode_half(left),
        );
        Ok(vec![right_result?, left_result?])
    } else {
        let resized = img.thumbnail(target_width, target_height);
        let (rw, rh) = (resized.width(), resized.height());
        let mut output = Vec::new();
        resized.write_to(&mut Cursor::new(&mut output), ImageFormat::Jpeg)?;
        Ok(vec![(output, rw, rh)])
    }
}

pub async fn build(mode: &FileMod, output_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    match mode {
        FileMod::EPUB(params) => super::epub::build(params, output_path).await,
        FileMod::CBZ(params) => super::cbz::build(params, output_path).await,
    }
}
