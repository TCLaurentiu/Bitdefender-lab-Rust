const LOAD_INDEX_API: &str = "http://127.0.0.1:8000/load";
const SEARCH_API: &str = "http://127.0.0.1:8000/search";

use std::io::{self, BufRead};
use reqwest;
use zip_indexer::{GenericResultError, SearchData, SearchResult};

fn main() -> GenericResultError<()>{
  println!("Available commands:
  load: loads the prebuilt index.mpk
  search (comma separated keywords): performs a search with the given keywords 
  exit: quits the tool
  max_length (integer): set maximum amount of returned search results
  min_score (float): set minimum score of returned search results
  ");

  let client = reqwest::blocking::Client::new();
  let mut max_length = 100;
  let mut min_score = 0.0;

  let stdin = io::stdin();
  for line in stdin.lock().lines() {
    let line = line?;
    if line.starts_with("exit"){
      break;
    }

    if line.starts_with("load"){
      let resp = reqwest::blocking::get(LOAD_INDEX_API);
      if let Err(err) = resp {
        println!("Error loading index: {:?}", err);
      } else {
        println!("Index succesfully loaded");
      }
      continue;
    }

    if line.starts_with("max_length"){
      match line.split(" ").last() {
        Some(length) => {
          if let Ok(length) = length.parse::<i32>() {
            max_length = length;
          } else {
            println!("Can't parse as integer");
          }
        }
        None => {
          println!("Must supply max_length");
        }
      }
      continue;
    }

    if line.starts_with("min_score"){
      match line.split(" ").last() {
        Some(score) => {
          if let Ok(score) = score.parse::<f64>() {
            min_score = score;
          } else {
            println!("Invalid command");
          }
        }
        None => {
          println!("Invalid command");
        }
      }
      continue;
    }

    let command_parts: Vec<&str> = line.split(" ").collect();
    if command_parts[0] != "search" {
      println!("Invalid command");
      continue;
    }

    let keywords = command_parts[1];
    let search_data = SearchData{
      terms: keywords.split(",").map(|keyword| String::from(keyword)).collect(),
      max_length: Some(max_length),
      min_score: Some(min_score),
    };

    let res: SearchResult = client.post(SEARCH_API).json(&search_data).send()?.json()?;
    if res.total == 0{
      println!("No results found");
    } else {
      for result in res.matches{
        println!("Found {}, with score {}", result.file_name, result.score);
      }
    }

  }
  Ok(())
}