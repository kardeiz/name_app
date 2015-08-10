extern crate docopt;

extern crate rusqlite;
extern crate r2d2;
extern crate r2d2_sqlite;

extern crate glob;
extern crate regex;
extern crate csv;
extern crate rustc_serialize;

extern crate iron;
extern crate persistent;
extern crate router;
extern crate urlencoded;
extern crate staticfile;
extern crate mount;

use std::env;
use glob::glob;
use std::path::{Path, PathBuf};
use regex::Regex;

use docopt::Docopt;

#[derive(RustcDecodable, RustcEncodable, Debug)]
pub struct Name {
  name: String,
  sex: String,
  year: String,
  number: i64
}

pub mod db {

  use regex::Regex;
  use glob::glob;
  use std::path::{Path, PathBuf};

  use r2d2_sqlite::SqliteConnectionManager;
  use r2d2::{self, Pool, PooledConnection};
  use rusqlite::SqliteConnection;
  use rusqlite::types::{ToSql, FromSql};

  use iron::typemap::Key;

  use csv;
  use super::Name;

  pub type SqlitePool = Pool<SqliteConnectionManager>;
  pub type SqlitePooledConnection = PooledConnection<SqliteConnectionManager>;

  pub struct AppDb;
  impl Key for AppDb { type Value = SqlitePool; }

  pub fn setup_connection_pool() -> SqlitePool {
    let manager = SqliteConnectionManager::new("test.db").unwrap();
    let config = r2d2::Config::default();
    r2d2::Pool::new(config, manager).unwrap()
  }

  pub fn build_table() {
    let stmts = [
      "CREATE TABLE IF NOT EXISTS names ( \
        id INTEGER PRIMARY KEY, \
        name TEXT NOT NULL, \
        sex TEXT NOT NULL, \
        year TEXT NOT NULL, \
        number INTEGER NOT NULL )".trim(),
      "CREATE INDEX IF NOT EXISTS names_name_idx ON names(name)",
      "CREATE INDEX IF NOT EXISTS names_sex_idx ON names(sex)",
      "CREATE INDEX IF NOT EXISTS names_year_idx ON names(year)"
    ];
    let pool = setup_connection_pool();
    let conn = pool.get().unwrap();
    for stmt in stmts.iter() {
      conn.execute(stmt, &[]).unwrap();
    }
  }

  pub fn load_data() {
    let rey = Regex::new(r"\d{4}").unwrap();
    let txts: Vec<PathBuf> = glob("data/*.txt").unwrap().map(|r| r.unwrap()).collect();
    
    let pool = setup_connection_pool();
    let conn = pool.get().unwrap();

    conn.execute("DELETE FROM names", &[]).unwrap();

    for txt in txts {
      let pn = txt.as_path().to_str().unwrap();
      println!("{:?}", pn);

      let year = rey.captures(pn).and_then(|caps| caps.at(0) ).unwrap();

      let mut rdr = csv::Reader::from_file(txt.as_path()).unwrap();      
      
      let tx = conn.transaction().unwrap();
      
      for record in rdr.decode() {
        let (name, sex, number): (String, String, String) = record.unwrap();
        conn.execute("INSERT INTO names (name, sex, year, number) \
          VALUES ($1, $2, $3, $4)",
          &[&name, &sex, &year, &number]).unwrap();
      }
      
      tx.commit();
    }
  }

  fn placeholders_for(len: usize) -> String {
    let mut first = true;
    (1..(len + 1)).fold(String::new(), |acc, item| 
      if first { 
        first = false;
        acc + "$" + &item.to_string()[..]
      } else {
        acc + ", $" + &item.to_string()[..]
      }
    )
  }

  pub fn rows_for(conn: SqlitePooledConnection, names: Vec<String>, sex: Option<String>) -> Vec<Name> {
    let phs = placeholders_for(names.len());    
    let mut args = names.clone();
    let _stmt = match sex {
      Some(sex_un) => {
        args.push(sex_un);        
        format!("SELECT name, sex, year, number \
          FROM names where name IN ({}) COLLATE NOCASE AND sex = ${} COLLATE NOCASE", 
          phs, names.len() + 1)
      },
      _ => {
        format!("SELECT name, sex, year, number \
          FROM names where name IN ({}) COLLATE NOCASE", phs)
      }
    };
    println!("{:?}", _stmt);
    let mut stmt = conn.prepare(&_stmt).unwrap();
    let pargs = args.iter().map(|s| s as &ToSql ).collect::<Vec<&ToSql>>();
    let rows = stmt.query(&pargs[..]).unwrap();
    let mut names = Vec::new();
    for _row in rows {
      let row =  _row.unwrap();
      let name = Name { 
        name: row.get(0),
        sex: row.get(1),
        year: row.get(2),
        number: row.get(3)
      };
      names.push(name);
    }
    names
  }

}

pub mod web {

  use std::path::{Path, PathBuf};

  use iron::prelude::*;
  use iron::{status};
  use iron::mime::Mime;
  use iron::typemap::Key;

  use mount::Mount;
  use staticfile::Static;
  use router::Router;
  use urlencoded::UrlEncodedQuery;

  use rustc_serialize::json;

  use persistent::{Write,Read};
  use super::db;


  fn get_parameters(req: &mut Request, name: &str) -> Vec<String> {
    req.get_ref::<UrlEncodedQuery>().ok()
      .and_then( |hashmap| 
        hashmap.get(name).map( |v| v.iter().map(|s| s.clone() ).collect() )
      ).unwrap_or(Vec::new())
  } 

  fn get_parameter(req: &mut Request, name: &str) -> Option<String> {
    req.get_ref::<UrlEncodedQuery>().ok()
      .and_then( |hashmap| 
        hashmap.get(name).and_then( |v| v.first().map(|s| s.clone() ) )
      )
  }

  pub fn run() {
    let pool = db::setup_connection_pool();
    
    let mut router = Router::new();
    router.get("/data", data);

    let mut mount = Mount::new();
    mount.mount("/", router);
    mount.mount("/static", Static::new(Path::new("./static/")));

    let mut middleware = Chain::new(mount);
    middleware.link(Read::<db::AppDb>::both(pool));

    Iron::new(middleware).http("0.0.0.0:3000").unwrap();
  }

  fn data(req: &mut Request) -> IronResult<Response> {

    let names = get_parameters(req, "name[]");

    println!("{:?}", names);
    let sex   = get_parameter(req, "sex");

    let mmtp = "application/json".parse::<Mime>().unwrap();
    
    let pool = req.get::<Read<db::AppDb>>().unwrap();
    let conn = pool.get().unwrap();

    let nrows = db::rows_for(conn, names, sex);
    let encoded = json::encode(&nrows).unwrap();
    Ok(Response::with((mmtp, status::Ok, encoded)))
  }

}




fn db_test() { }

static USAGE: &'static str = "
Usage:
  name_app <task>
";

#[derive(Debug, RustcDecodable)]
struct Args {
 arg_task: String
}


fn main() {

  let args: Args = Docopt::new(USAGE)
    .and_then(|d| d.decode())
    .unwrap_or_else(|e| e.exit());

  println!("{:?}", args);

  match args.arg_task.as_ref() {
    "run" => web::run(),
    _ => panic!("no")
  }

  // match env::var("TASK") {
  //   Ok(ref val) if val == "BUILD" => db::build_table(),
  //   Ok(ref val) if val == "LOAD"  => db::load_data(),
  //   Ok(ref val) if val == "RUN"   => web::run(),
  //   Ok(ref val) if val == "TEST"   => db_test(),
  //   _ => { println!("Error"); }
  // }
}
