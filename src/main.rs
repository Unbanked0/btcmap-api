use actix_web::middleware::NormalizePath;
use actix_web::web::scope;
use actix_web::web::Data;
extern crate core;

mod auth;
mod controller;
mod db;
mod generate_android_icons;
mod generate_report;
mod model;
mod sync;
mod sync_users;

use std::env;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::sync::Mutex;

use actix_web::middleware::Logger;
use actix_web::{App, HttpServer};
use directories::ProjectDirs;
use rusqlite::Connection;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }

    if env::var("ADMIN_TOKEN").is_err() && cfg!(debug_assertions) {
        env::set_var("ADMIN_TOKEN", "debug");
    }

    env_logger::init();

    log::info!("Initializing BTC Map API");

    if env::var("RUST_BACKTRACE").is_err() {
        log::info!("Activating RUST_BACKTRACE");
        env::set_var("RUST_BACKTRACE", "1");
    }

    let args: Vec<String> = env::args().collect();
    log::info!("Got {} arguments ({:?})", args.len(), args);
    let mut db_conn = Connection::open(get_db_file_path()).unwrap();

    match args.len() {
        1 => {
            if let Err(err) = db::migrate(&mut db_conn) {
                log::error!("Migration faied: {err}");
                std::process::exit(1);
            }

            let db_conn = Data::new(Mutex::new(db_conn));

            log::info!("Starting HTTP server");
            HttpServer::new(move || {
                App::new()
                    .wrap(Logger::default())
                    .wrap(NormalizePath::trim())
                    .app_data(db_conn.clone())
                    .service(
                        scope("elements")
                            .service(controller::element_v2::get)
                            .service(controller::element_v2::get_by_id)
                            .service(controller::element_v2::post_tags),
                    )
                    .service(
                        scope("events")
                            .service(controller::event_v2::get)
                            .service(controller::event_v2::get_by_id),
                    )
                    .service(
                        scope("users")
                            .service(controller::user_v2::get)
                            .service(controller::user_v2::get_by_id)
                            .service(controller::user_v2::post_tags),
                    )
                    .service(
                        scope("areas")
                            .service(controller::area_v2::post)
                            .service(controller::area_v2::get)
                            .service(controller::area_v2::get_by_id)
                            .service(controller::area_v2::post_tags),
                    )
                    .service(
                        scope("reports")
                            .service(controller::report_v2::get)
                            .service(controller::report_v2::get_by_id),
                    )
                    .service(
                        scope("v2")
                            .service(
                                scope("elements")
                                    .service(controller::element_v2::get)
                                    .service(controller::element_v2::get_by_id)
                                    .service(controller::element_v2::post_tags),
                            )
                            .service(
                                scope("events")
                                    .service(controller::event_v2::get)
                                    .service(controller::event_v2::get_by_id),
                            )
                            .service(
                                scope("users")
                                    .service(controller::user_v2::get)
                                    .service(controller::user_v2::get_by_id)
                                    .service(controller::user_v2::post_tags),
                            )
                            .service(
                                scope("areas")
                                    .service(controller::area_v2::post)
                                    .service(controller::area_v2::get)
                                    .service(controller::area_v2::get_by_id)
                                    .service(controller::area_v2::post_tags),
                            )
                            .service(
                                scope("reports")
                                    .service(controller::report_v2::get)
                                    .service(controller::report_v2::get_by_id),
                            ),
                    )
            })
            .bind(("127.0.0.1", 8000))?
            .run()
            .await
        }
        _ => {
            let db_conn = Connection::open(get_db_file_path()).unwrap();

            match args.get(1).unwrap().as_str() {
                "db" => {
                    db::cli_main(&args[2..], db_conn);
                }
                "sync" => {
                    sync::sync(db_conn).await;
                }
                "sync-users" => {
                    sync_users::sync(db_conn).await;
                }
                "generate-report" => {
                    generate_report::generate_report(db_conn).await;
                }
                "generate-android-icons" => {
                    generate_android_icons::generate_android_icons(db_conn).await;
                }
                _ => {
                    log::error!("Unknown action");
                    std::process::exit(1);
                }
            }

            Ok(())
        }
    }
}

fn get_db_file_path() -> PathBuf {
    let project_dirs = get_project_dirs();

    if !project_dirs.data_dir().exists() {
        create_dir_all(project_dirs.data_dir()).unwrap()
    }

    project_dirs.data_dir().join("btcmap.db")
}

fn get_project_dirs() -> ProjectDirs {
    return ProjectDirs::from("org", "BTC Map", "BTC Map").unwrap();
}
