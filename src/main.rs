mod config;
mod utils;

use clap::{ArgGroup, Parser};
use tracing::error;
use std::{error::Error, path::Path};
use strum::IntoEnumIterator;
use plotly::ImageFormat as PlotlyImageFormat;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

use utils::{
    analyze_exposure, calculate_ter, parse_portfolio, parse_securities, plot_grid, Conf, Exposure,
};

#[derive(Debug, Copy, Clone, clap::ValueEnum)]
pub enum ImageFormat {
   PNG,
   JPEG,
   WEBP,
   SVG,
   PDF,
   EPS
}

impl Into<PlotlyImageFormat> for ImageFormat {
    fn into(self) -> PlotlyImageFormat {
        match self {
            ImageFormat::PNG => PlotlyImageFormat::PNG,
            ImageFormat::JPEG => PlotlyImageFormat::JPEG,
            ImageFormat::WEBP => PlotlyImageFormat::WEBP,
            ImageFormat::SVG => PlotlyImageFormat::SVG,
            ImageFormat::PDF => PlotlyImageFormat::PDF,
            ImageFormat::EPS => PlotlyImageFormat::EPS,
        }
    }
}

/// Simple portfolio holdings analyzer
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[clap(group(
            ArgGroup::new("currency")
                .args(&["set_currency", "eur", "usd"]),
        ))]
struct Args {
    /// CSV file containing asset allocation information about all the securities in your portfolio.
    /// CSV file format is `ISIN,Name,Ticker,TER,Holding,HoldingWeight,Sector,SectorWeight,Country,CountryWeight,Region,RegionWeight`
    #[arg()]
    securities: String,

    /// CSV file containing information about your portfolio securities distribution.
    /// CSV file format is `ISIN,Amount` where amount is in your currency or `ISIN,Weight` where weight is the percentage amount
    #[arg()]
    portfolio: String,

    /// Save the output as a static image with size of 1920x1080
    #[arg(short = 'i', long)]
    save_image: bool,

    #[arg(short = 'f', long, value_enum, default_value_t=ImageFormat::PNG)]
    image_format: ImageFormat,

    /// Scale the output image up or down
    #[arg(short = 's', long, default_value_t = 1.0)]
    image_scale: f64,

    /// Save output to this folder. If none is provided, save output to the same folder as the portfolio
    #[arg(short = 'o', long)]
    output_folder: Option<String>,

    /// Display the fully rendered graphs in the default system browser
    #[arg(short, long)]
    display: bool,

    /// Portfolio currency is Euro [default: true]
    #[arg(long)]
    eur: bool,

    /// Portfolio currency is USD
    #[arg(long)]
    usd: bool,

    /// Define custom portfolio currency
    #[arg(long, value_name = "CURRENCY")]
    set_currency: Option<String>,

    /// Limit the number of data points per graph
    #[arg(short = 'l', long, default_value_t = 25)]
    limit: usize,

    /// Logging filter
    #[arg(long, env = "RUST_LOG", default_value = "info")]
    log_filter: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_filter(EnvFilter::new(args.log_filter)),
        )
        .init();

    let currency = if let Some(cur) = args.set_currency.as_deref() {
        cur.to_string()
    } else {
        let (eur, usd) = (args.eur, args.usd);
        match (eur, usd) {
            (_, true) => "$".to_owned(),
            _ => "€".to_owned(),
        }
    };

    let securities = match parse_securities(args.securities) {
        Ok(securities) => securities,
        Err(err) => {
            error!("{}", err);
            panic!("Errors occured")
        },
    };
    
    let (total, portfolio) = parse_portfolio(&args.portfolio)?;

    let output_file_name = Path::new(&args.portfolio)
        .file_stem()
        .expect("Portfolio file name")
        .to_os_string();
    let output_folder = if let Some(folder) = args.output_folder {
        folder
    } else {
        Path::new(&args.portfolio)
            .parent()
            .expect("Portfolio file path")
            .to_string_lossy()
            .to_string()
    };

    let mut exposures = Vec::new();
    for exposure in Exposure::iter() {
        let result = analyze_exposure(&securities, &portfolio, exposure)?;
        exposures.push((exposure, result));
    }
    let ter = calculate_ter(&securities, &portfolio)?;
    let conf = Conf {
        limit: args.limit,
        currency,
        display: args.display,
        image: args.save_image,
        image_scale: args.image_scale,
        image_format: args.image_format,
        output_file_name,
        output_folder,
    };
    plot_grid(exposures, total, ter, &conf)?;
    Ok(())
}
