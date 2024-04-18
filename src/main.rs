use std::fs;
use std::fs::File;
use std::fs::DirEntry;
use std::io::prelude::*;
use std::io::BufReader;
use serde::Deserialize;
use serde::Serialize;
use std::env;
use std::collections::HashMap;
use tqdm::tqdm;

fn is_zip(path: &DirEntry) -> bool {
    path.file_name()
        .to_str()
        .map_or(false, |s| s.ends_with(".zip"))
}

#[derive(Debug, Serialize, Deserialize)]
struct FileData{
    name: String,
    files: Vec<String>
}

fn get_zip_contents(reader: impl Read + Seek) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut zip = zip::ZipArchive::new(reader)?;

    let mut filenames: Vec<String> = Vec::new();

    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        filenames.push(file.name().to_owned());
    }

    Ok(filenames)
}

fn find_zip_data_in_folder(folder_path: String) -> Result<Vec<FileData>, Box<dyn std::error::Error>>{
    let mut all_zips_data: Vec<FileData> = Vec::new();
    let paths = fs::read_dir(folder_path).unwrap();
    for entry in paths {
        let entry = entry?;
        let path = entry.path();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        if is_zip(&entry) {
            let filenames:Vec<String> = get_zip_contents(File::open(&path)?)?;
            all_zips_data.push(FileData{
                name: filename,
                files: filenames
            });
        }
    }
    Ok(all_zips_data)
}

fn dump_and_save_zip_data(all_zips_data: Vec<FileData>, file_path: String) -> Result<(), Box<dyn std::error::Error>>{
    let mut writer = File::create(file_path)?;
    for fd in all_zips_data{
        serde_json::to_writer(&writer, &fd)?;
        writer.write_all(&[b'\n'])?;
    }
    Ok(())
}

type Term = String;
type DocumentId = String;

type DocumentMap = HashMap<Term, Vec<DocumentId>>;

fn read_zips_data_from_json(file_path: String) -> Result<DocumentMap, Box<dyn std::error::Error>> {
    let mut doc_map:DocumentMap = DocumentMap::new();

    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    for line in tqdm(reader.lines()){
        let zip_data:FileData = serde_json::from_str(&line?)?;
        let file_names = zip_data.files;
        for file_name in file_names.iter(){
            for path_part in file_name.split("/"){
                let docid_vec = doc_map.entry(path_part.to_string()).or_insert(vec![]);
                if !docid_vec.contains(&(zip_data.name)){
                    docid_vec.push(zip_data.name.clone());
                }
            }
        }
    }

    Ok(doc_map)
}

#[allow(dead_code)]
fn print_zips_data(zips_data: DocumentMap){
    for (term, filenames) in &zips_data{
        println!("term {} found in: {:?}", term, filenames);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>>{

    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        println!("Invalid arguments. Valid usage is -- read 'ndjson_file_name' or -- write 'folder_path'");
        return Ok(())
    }

    let query = &args[1];
    let file_name =  &args[2];

    match query.as_str() {
        "write" => {
            let all_zips_data = find_zip_data_in_folder(file_name.to_string());
            dump_and_save_zip_data(all_zips_data?, String::from("zips.ndjson"))?;
        },
        "read" => {
            let zip_data = read_zips_data_from_json(file_name.to_string())?;
            println!("Succesfully read ndJSON file: {} entries", zip_data.len());
            // print_zips_data(zip_data);
        },
        _ => {
            println!("Invalid option")
        }
    }

    Ok(())

}
