#![allow(clippy::blocks_in_conditions)]

use std::{
    fs::{ File, DirEntry },
    io::{ prelude::*, BufReader},
    sync::{ Arc, RwLock },
    fs,
    collections::HashSet,
};

use rmp_serde:: Serializer;
use serde::{ Serialize, Deserialize };

use tqdm::tqdm;
use uuid::Uuid;

use bimap::BiMap;
use fxhash::FxHashMap;

#[macro_use]
extern crate rocket;
use rocket::{ fs::{ FileServer, TempFile }, form::Form, serde::json::Json, State };
use zip_indexer::{GenericResultError, SearchData, SearchMatch, SearchResult};

type Term = String;
type DocumentId = u64;

#[derive(Deserialize, Serialize)]
struct TermData {
    docset: FxHashMap<DocumentId, u64>,
    idf: f64,
}

type DocumentMap = FxHashMap<Term, TermData>;
type ResultsMap = FxHashMap<DocumentId, f64>;

// all the data we generate after parsing a folder of zip files
#[derive(Deserialize, Serialize)]
struct ZipsData {
    doc_map: DocumentMap,
    docid_to_int: BiMap<String, u64>,
    doc_size: FxHashMap<DocumentId, u64>,
}

type ScoreFunction = dyn Fn(
    &Vec<String>,
    &DocumentMap,
    &FxHashMap<DocumentId, u64>,
    DocumentId
) -> GenericResultError<f64>;

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

fn read_and_dump_zips(folder_path: String, json_file_path: String) -> GenericResultError<()> {
    let all_zips_data = find_zip_data_in_folder(folder_path)?;
    dump_and_save_zip_data(all_zips_data, json_file_path)?;
    Ok(())
}

fn read_zips_data_from_json(file_path: String) -> GenericResultError<ZipsData> {
    let mut doc_map: DocumentMap = DocumentMap::default();
    let mut docid_to_int: BiMap<String, DocumentId> = BiMap::new();

    let mut document_size: FxHashMap<DocumentId, u64> = FxHashMap::default();

    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    let mut zip_name_id = 0;

    let mut words_in_current_doc = HashSet::new();

    for line in tqdm(reader.lines()) {
        let zip_data: FileData = serde_json::from_str(&line?)?;
        let file_names = zip_data.files;
        docid_to_int.insert(zip_data.name, zip_name_id);

        for file_name in file_names.iter() {
            for path_part in file_name.split('/') {
                let docid_set = doc_map.entry(path_part.to_string()).or_insert(TermData {
                    docset: FxHashMap::default(),
                    idf: 0f64,
                });
                *docid_set.docset.entry(zip_name_id).or_insert(0) += 1;
                words_in_current_doc.insert(path_part);
            }
        }
        document_size.entry(zip_name_id).or_insert(words_in_current_doc.len() as u64);
        words_in_current_doc = HashSet::new();
        zip_name_id += 1;
    }

    let doc_count = zip_name_id as f64;
    for (_, termdata) in doc_map.iter_mut() {
        let count = termdata.docset.len() as f64;
        termdata.idf = ((doc_count - count + 0.5) / (count + 0.5) + 1.0).ln();
    }

    Ok(ZipsData {
        doc_map,
        docid_to_int,
        doc_size: document_size,
    })
}

#[allow(dead_code)]
fn print_zips_data(zips_data: DocumentMap) {
    for (term, termdata) in &zips_data {
        println!("term {} found in: {:?}", term, termdata.docset);
    }
}

#[allow(dead_code)]
fn print_data_statistics(data: &DocumentMap) {
    let term_count = data.len();
    let mut term_docid_pairs = 0;
    for (term, termdata) in data {
        term_docid_pairs += termdata.docset.len();
        println!("Term {} with IDF = {}", term, termdata.idf);
        for (file, occurence_count) in &termdata.docset {
            println!("Term appeared in document {} {} times", file, occurence_count);
        }
    }
    println!("{} terms and {} term-docid pairs", term_count, term_docid_pairs);
}

fn run_search(
    search_terms: &Vec<String>,
    doc_map: &DocumentMap,
    doc_size: &FxHashMap<DocumentId, u64>,
    score_function: &ScoreFunction
) -> GenericResultError<ResultsMap> {
    let mut search_results: ResultsMap = FxHashMap::default();

    let document_count = doc_size.len();

    for document_id in 0..document_count {
        let score = score_function(search_terms, doc_map, doc_size, document_id as u64)?;
        search_results.entry(document_id as u64).or_insert(score);
    }

    Ok(search_results)
}

fn bm25_score_function(
    search_terms: &Vec<String>,
    doc_map: &DocumentMap,
    doc_size: &FxHashMap<DocumentId, u64>,
    document_id: DocumentId
) -> GenericResultError<f64> {
    let k1 = 1.2;
    let b = 0.75;

    let mut score = 0f64;
    let mut mean_size = 0f64;
    let doc_count = doc_size.len() as f64;

    for document_size in doc_size.values() {
        mean_size += *document_size as f64;
    }

    mean_size /= doc_count;

    let default_idf = ((doc_count + 0.5) / 0.5 + 1.0).ln();

    for search_term in search_terms {
        let idf = match doc_map.contains_key(search_term) {
            false => default_idf,
            true => doc_map.get(search_term).unwrap().idf,
        };
        let occurence_in_doc = match doc_map.contains_key(search_term) {
            false => 0f64,
            true =>
                *doc_map.get(search_term).unwrap().docset.get(&document_id).unwrap_or(&0) as f64,
        };
        let current_doc_size = *doc_size.get(&document_id).unwrap() as f64;
        let numerator = occurence_in_doc * (k1 + 1.0f64);
        let denumerator = occurence_in_doc + k1 * (1.0f64 - b + (b * current_doc_size) / mean_size);
        let fraction = numerator / denumerator;
        score += idf * fraction;
    }

    Ok(score)
}

fn order_results_map(results_map: &ResultsMap) -> GenericResultError<Vec<(DocumentId, f64)>> {
    let mut results_vec = results_map
        .iter()
        .map(|(doc_id, freq)| (*doc_id, *freq))
        .collect::<Vec<(DocumentId, f64)>>();
    results_vec.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok(results_vec)
}

#[allow(dead_code)]
fn print_search_results(
    results_vec: &[(DocumentId, f64)],
    docid_to_int: &BiMap<String, u64>,
    search_terms: &Vec<String>,
    limit: usize
) {
    println!("For search terms:");
    for search_term in search_terms {
        print!("{} ", search_term);
    }
    println!();
    println!("Found the following results:");
    for (doc_id, score) in results_vec.iter().take(limit) {
        println!("Zip file {}, with score {}", docid_to_int.get_by_right(doc_id).unwrap(), score);
    }
}

#[allow(dead_code)]
fn print_document_size(document_size: &FxHashMap<DocumentId, u64>) {
    for (doc_id, size) in document_size {
        println!("Document {} contains {} unique items", doc_id, size);
    }
}

struct ServerState {
    index: ZipsData,
}

#[get("/")]
fn index() -> &'static str {
    "Hello, world!"
}

#[derive(FromForm)]
struct Upload<'r> {
    file: TempFile<'r>,
}

#[post("/upload", data = "<upload>")]
async fn upload(mut upload: Form<Upload<'_>>) -> Result<(), String> {
    let id = Uuid::new_v4();
    let mut path = "static/zips/".to_owned();
    let name = format!("{}-{}.zip", upload.file.name().unwrap_or("_"), id.simple());
    path.push_str(&name);
    upload.file.persist_to(path).await.map_err(|err| format!("Error: {err:#}"))?;
    read_and_dump_zips(String::from("static/zips"), String::from("static/index/data.json")).map_err(
        |err| format!("Error: {err:#}")
    )?;
    Ok(())
}

#[post("/build")]
async fn build(server_state: &State<Arc<RwLock<ServerState>>>) -> Result<(), String> {
    let index = read_zips_data_from_json(String::from("static/index/data.json")).map_err(|err|
        format!("Error: {err:#}")
    )?;
    let mut server_state = server_state.write().map_err(|err| format!("Error: {err:#}"))?;
    server_state.index = index;
    Ok(())
} 

#[post("/clear")]
async fn clear() -> Result<(), String> {
    let path = "static/zips";
    fs::remove_dir_all(path).map_err(|err| format!("Erorr: {err:#}"))?;
    fs::create_dir(path).map_err(|err| format!("Error: {err:#}"))?;
    Ok(())
}



#[post("/dump")]
fn dump(
    server_state: &State<Arc<RwLock<ServerState>>>
) -> Result<(), String> {
    let server_state = server_state.read().map_err(|err| format!("Error :{err:#}"))?;
    let index = &server_state.index;

    let mut buf = Vec::new();
    index.serialize(&mut Serializer::new(&mut buf)).map_err(|err| format!("Error :{err:#}"))?;

    let msgpack_index_path = String::from("static/index/index.mpk");
    fs::write(msgpack_index_path, buf).map_err(|err| format!("Error :{err:#}"))?;

    Ok(())

}

#[derive(Serialize)]
struct Zips {
    zip_names: Vec<String>
}
#[get("/get_zips")]
fn get_zips() -> Result<Json<Zips>, String> {
    let folder_path = "static/zips";
    let paths: Vec<String> = fs::read_dir(folder_path).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
    Ok(Json(Zips {
        zip_names: paths
    }))
}


#[derive(Deserialize)]
struct DeleteRequest{
    zip_id: u32,
    zip_name: String,
}

#[post("/delete_zip", data = "<req>")]
fn delete_zip(
    req: Json<DeleteRequest>,
) -> Result<(), String> {
    let folder_path = "static/zips";
    let zip_name = req.zip_name.clone();
    let zip_id = req.zip_id;

    let nth_in_folder = fs::read_dir(folder_path).unwrap().nth(zip_id as usize).unwrap().unwrap().file_name().into_string().unwrap();

    if nth_in_folder == zip_name {
        fs::remove_file(format!("{}/{}", folder_path, nth_in_folder)).map_err(|err| format!("Error: {err:#}"))?;
        return Ok(());
    } else {
        return Err(String::from("Error deleting file"));
    }
}

#[post("/load")]
fn load(
    server_state: &State<Arc<RwLock<ServerState>>>
) -> Result<(), String> {
    let msgpack_index_path = String::from("static/index/index.mpk");
    let buf = fs::read(msgpack_index_path).map_err(|err| format!("Error :{err:#}"))?;
    let index = rmp_serde::from_slice(&buf).map_err(|err| format!("Error :{err:#}"))?;

    let mut server_state = server_state.write().map_err(|err| format!("Error: {err:#}"))?;
    server_state.index = index;
    Ok(())
}

#[post("/search", data = "<req>")]
fn search(
    req: Json<SearchData>,
    server_state: &State<Arc<RwLock<ServerState>>>
) -> Result<Json<SearchResult>, String> {
    let terms = req.terms.clone();
    let server_state = server_state.read().map_err(|err| format!("Error: {err:#}"))?;

    let doc_map = &server_state.index.doc_map;
    let docid_to_int = &server_state.index.docid_to_int;
    let document_size = &server_state.index.doc_size;

    let results_map = run_search(&terms, doc_map, document_size, &bm25_score_function).map_err(|err|
        format!("Error: {err:#}")
    )?;
    let ordered_results_map = order_results_map(&results_map).map_err(|err|
        format!("Error: {err:#}")
    )?;
    let matches: Vec<SearchMatch> = ordered_results_map
        .iter()
        .map(|(doc_id, score)| {
            SearchMatch {
                file_name: docid_to_int.get_by_right(doc_id).unwrap().to_string(),
                score: *score,
            }
        })
        .collect();

    let total = matches.len();

    let max_length = req.max_length.unwrap_or(total as i32);
    let min_score = req.min_score.unwrap_or(0.0);

    let filtered_matches: Vec<SearchMatch> = matches
        .into_iter()
        .filter(|search_match| search_match.score > min_score)
        .take(max_length as usize)
        .collect();
    let total = filtered_matches.len();

    Ok(
        Json(SearchResult {
            matches: filtered_matches,
            total,
        })
    )
}

#[rocket::main]
async fn main() -> eyre::Result<()> {

    let empty_index = ZipsData{
        doc_map: DocumentMap::default(),
        docid_to_int: BiMap::new(),
        doc_size: FxHashMap::default(),
    };

    let server_state = Arc::new(
        RwLock::new(ServerState {
            index: empty_index
        })
    );

    rocket
        ::build()
        .manage(server_state)
        .mount("/", routes![index, upload, build, clear, search, dump, load, get_zips, delete_zip])
        .mount("/dashboard", FileServer::from("static"))
        .ignite().await?
        .launch().await?;

    Ok(())
}
