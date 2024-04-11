use std::fs;
use std::fs::File;
use std::fs::DirEntry;
use std::io::prelude::*;

fn is_zip(path: &DirEntry) -> bool {
    path.file_name()
        .to_str()
        .map_or(false, |s| s.ends_with(".zip"))
}


fn list_zip_contents(reader: impl Read + Seek) -> zip::result::ZipResult<()> {
    let mut zip = zip::ZipArchive::new(reader)?;

    let mut total_uncompressed_size = 0;
    let mut total_compressed_size = 0;

    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        total_uncompressed_size += file.size();
        total_compressed_size += file.compressed_size();
        println!("Filename: {}", file.name());
    }
    
    println!("Compression rate {}", total_compressed_size as f32 / total_uncompressed_size as f32);

    println!("---");

    Ok(())
}

fn print_contents_of_zip_files(folder_path: String) {
    let paths = fs::read_dir(folder_path).unwrap();
    for path in paths {
        let path = path.unwrap();
        if is_zip(&path) {
            match list_zip_contents(File::open(&path.path()).unwrap()) {
                Err(_) => println!("Error reading file"),
                Ok(_) => ()
            }
        }
    }
}

fn main() {

    print_contents_of_zip_files(String::from("data/folder"));

}
