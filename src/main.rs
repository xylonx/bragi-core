use actix_web::{
    middleware::Logger,
    web::{self, Json, Query},
    App, HttpServer,
};

use bragi_core::{
    scraper::{
        Provider, ScrapeItem, ScrapeType, ScraperManager, SongCollection, Stream, WithProvider,
    },
    settings::Settings,
};
use clap::Parser;
use serde::Deserialize;
use tracing::info;

#[derive(Clone)]
struct Context {
    manager: ScraperManager,
    settings: Settings,
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// path of config file
    #[arg(short, long)]
    config: Option<String>,
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let arg = Args::parse();
    let settings = Settings::new(arg.config, None)?;

    let ctx = Context {
        manager: ScraperManager::try_from_settings(&settings).await?,
        settings: settings.clone(),
    };

    Ok(HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(ctx.clone()))
            .wrap(Logger::default())
            .service(
                web::scope("/api/v1")
                    .service(
                        web::scope("/scrape")
                            .route("/suggest", web::get().to(suggest_handler))
                            .route("/search", web::get().to(search_handler))
                            .route("/collection", web::get().to(collection_handler))
                            .route("/stream", web::get().to(stream_handler)),
                    )
                    .service(
                        web::scope("/stream").route("/spotify", web::get().to(stream_handler)),
                    ),
            )
    })
    .bind((settings.application.host, settings.application.port))?
    .run()
    .await?)
}

// async fn validator() -> Result<ServiceRequest, (actix_web::error::Error, ServiceRequest)> {

// }

#[derive(Debug, Deserialize)]
struct SuggestParam {
    keyword: String,
}

async fn suggest_handler(
    param: Query<SuggestParam>,
    ctx: web::Data<Context>,
) -> Json<Vec<WithProvider<String>>> {
    info!("[Handler] suggest with param: {:?}", param);

    Json(ctx.manager.suggest(param.keyword.clone()).await)
}

#[derive(Debug, Deserialize)]
struct SearchParam {
    keyword: String,
    #[serde(default = "default_type")]
    t: ScrapeType,
}

fn default_type() -> ScrapeType {
    ScrapeType::All
}

async fn search_handler(
    param: Query<SearchParam>,
    ctx: web::Data<Context>,
) -> Json<Vec<WithProvider<ScrapeItem>>> {
    info!("[Handler] search with param: {:?}", param);

    Json(
        ctx.manager
            .search(param.keyword.clone(), param.t.clone())
            .await,
    )
}

#[derive(Debug, Deserialize)]
struct CollectionParam {
    provider: Provider,
    id: String,
}

async fn collection_handler(
    param: Query<CollectionParam>,
    ctx: web::Data<Context>,
) -> actix_web::Result<Json<SongCollection>> {
    info!("[Handler] collection detail with param: {:?}", param);

    Ok(Json(
        ctx.manager
            .collection_detail(param.id.clone(), param.provider.clone())
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?,
    ))
}

#[derive(Debug, Deserialize)]
struct StreamParam {
    provider: Provider,
    id: String,
}

async fn stream_handler(
    param: Query<StreamParam>,
    ctx: web::Data<Context>,
) -> actix_web::Result<Json<Vec<Stream>>> {
    info!("[Handler] stream with param: {:?}", param);

    Ok(Json(
        ctx.manager
            .stream(param.id.clone(), param.provider.clone())
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?,
    ))
}
