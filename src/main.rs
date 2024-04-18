use std::fs;
use std::fs::File;
use std::fs::DirEntry;
use std::io::prelude::*;
use std::io::BufReader;
use serde::Deserialize;
use serde::Serialize;
use std::env;
use std::collections::{ HashMap, HashSet };
use tqdm::tqdm;
use bimap::BiMap;

type Term = String;
type DocumentId = u64;

type DocumentMap = HashMap<Term, HashSet<DocumentId>>;
type ResultsMap = HashMap<DocumentId, u64>;
type GenericResultError<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Serialize, Deserialize)]
struct FileData {
    name: String,
    files: Vec<String>,
}

fn is_zip(path: &DirEntry) -> bool {
    path.file_name()
        .to_str()
        .map_or(false, |s| s.ends_with(".zip"))
}

fn get_zip_contents(reader: impl Read + Seek) -> GenericResultError<Vec<String>> {
    let mut zip = zip::ZipArchive::new(reader)?;

    let mut filenames: Vec<String> = Vec::new();

    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        filenames.push(file.name().to_owned());
    }

    Ok(filenames)
}

fn find_zip_data_in_folder(folder_path: String) -> GenericResultError<Vec<FileData>> {
    let mut all_zips_data: Vec<FileData> = Vec::new();
    let paths = fs::read_dir(folder_path).unwrap();
    for entry in paths {
        let entry = entry?;
        let path = entry.path();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        if is_zip(&entry) {
            let filenames: Vec<String> = get_zip_contents(File::open(&path)?)?;
            all_zips_data.push(FileData {
                name: filename,
                files: filenames,
            });
        }
    }
    Ok(all_zips_data)
}

fn dump_and_save_zip_data(
    all_zips_data: Vec<FileData>,
    file_path: String
) -> GenericResultError<()> {
    let mut writer = File::create(file_path)?;
    for fd in all_zips_data {
        serde_json::to_writer(&writer, &fd)?;
        writer.write_all(&[b'\n'])?;
    }
    Ok(())
}

fn read_zips_data_from_json(
    file_path: String
) -> GenericResultError<(DocumentMap, BiMap<String, u64>)> {
    let mut doc_map: DocumentMap = DocumentMap::new();
    let mut docid_to_int: BiMap<String, u64> = BiMap::new();

    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    let mut zip_name_id = 0;

    for line in tqdm(reader.lines()) {
        let zip_data: FileData = serde_json::from_str(&line?)?;
        let file_names = zip_data.files;
        docid_to_int.insert(zip_data.name, zip_name_id);
        for file_name in file_names.iter() {
            for path_part in file_name.split("/") {
                let docid_set = doc_map.entry(path_part.to_string()).or_insert(HashSet::new());
                docid_set.insert(zip_name_id);
            }
        }
        zip_name_id += 1;
    }

    Ok((doc_map, docid_to_int))
}

#[allow(dead_code)]
fn print_zips_data(zips_data: DocumentMap) {
    for (term, filenames) in &zips_data {
        println!("term {} found in: {:?}", term, filenames);
    }
}

fn print_data_statistics(data: &DocumentMap) {
    let term_count = data.len();
    let mut term_docid_pairs = 0;
    for (_, filenames) in data {
        term_docid_pairs += filenames.len();
    }
    println!("{} terms and {} term-docid pairs", term_count, term_docid_pairs);
}

fn search(search_terms: &Vec<&str>, doc_map: &DocumentMap) -> GenericResultError<ResultsMap> {
    let mut search_results: ResultsMap = HashMap::new();

    for term in search_terms {
        for file_id in doc_map.get(*term).unwrap() {
            search_results.entry(*file_id).and_modify(|count| {
                *count += 1;
            }).or_insert(1);
        }
    }

    Ok(search_results)
}

fn order_results_map(results_map: &ResultsMap) -> GenericResultError<Vec<(DocumentId, u64)>> {
    let mut results_vec = results_map.iter().map(|(doc_id, freq)| (*doc_id, *freq)).collect::<Vec<(DocumentId, u64)>>();
    results_vec.sort_by(|a, b| (*b).1.cmp(&(*a).1));
    Ok(results_vec)
}

fn print_search_results(results_vec: &Vec<(DocumentId, u64)>, docid_to_int: &BiMap<String, u64>, search_terms: &Vec<&str>){
    let search_term_count = search_terms.len();
    for (doc_id, frequency) in results_vec{
        println!("Zip file {}, {}/{} terms are present", docid_to_int.get_by_right(doc_id).unwrap(), frequency, search_term_count);
    }
}

fn main() -> GenericResultError<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        println!(
            "Invalid arguments. Valid usage is -- read 'ndjson_file_name' or -- write 'folder_path'"
        );
        return Ok(());
    }

    let query = &args[1];
    let file_name = &args[2];

    match query.as_str() {
        "write" => {
            let all_zips_data = find_zip_data_in_folder(file_name.to_string());
            dump_and_save_zip_data(all_zips_data?, String::from("zips.ndjson"))?;
        }
        "read" => {
            let search_terms = vec!["lombok", "AUTHORS", "README.md"];
            let (doc_map, docid_to_int) = read_zips_data_from_json(file_name.to_string())?;
            print_data_statistics(&doc_map);
            let results_map = search(&search_terms, &doc_map)?;
            let ordered_results_map = order_results_map(&results_map)?;
            print_search_results(&ordered_results_map, &docid_to_int, &search_terms);
        }
        _ => {
            println!("Invalid option");
        }
    }

    Ok(())
}
