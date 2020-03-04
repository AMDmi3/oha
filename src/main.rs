use clap::Clap;
use url::Url;

mod printer;
mod work;

struct ParseDuration(std::time::Duration);

impl std::str::FromStr for ParseDuration {
    type Err = parse_duration::parse::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_duration::parse(s).map(ParseDuration)
    }
}

#[derive(Clap)]
#[clap(version = clap::crate_version!(), author = clap::crate_authors!())]
struct Opts {
    #[clap(help = "Target URL.")]
    url: String,
    #[clap(help = "Number of requests.", short = "n", default_value = "200")]
    n_requests: usize,
    #[clap(help = "Number of workers.", short = "c", default_value = "50")]
    n_workers: usize,
    #[clap(help = "Duration.\nExamples: -z 10s -z 3m.", short = "z")]
    duration: Option<ParseDuration>,
    #[clap(help = "Query per second limit.", short = "q")]
    query_per_second: Option<usize>,
}

pub struct RequestResult {
    duration: std::time::Duration,
    status: reqwest::StatusCode,
    len_bytes: usize,
}

async fn request(
    client: reqwest::Client,
    url: Url,
    reporter: tokio::sync::mpsc::UnboundedSender<anyhow::Result<RequestResult>>,
) -> Result<(), tokio::sync::mpsc::error::SendError<anyhow::Result<RequestResult>>> {
    let result = async move {
        let s = std::time::Instant::now();
        let resp = client.get(url.clone()).send().await?;
        let status = resp.status();
        let len_bytes = resp.bytes().await?.len();
        let duration = std::time::Instant::now() - s;
        Ok::<_, anyhow::Error>(RequestResult {
            duration,
            status,
            len_bytes,
        })
    }
    .await;
    reporter.send(result)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut opts: Opts = Opts::parse();
    let url = Url::parse(opts.url.as_str())?;
    let client = reqwest::Client::new();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let realtime_tui = tokio::spawn(async move {
        let mut all = Vec::new();
        while let Some(report) = rx.recv().await {
            // TODO: Some tui here
            all.push(report);
        }
        all
    });

    let start = std::time::Instant::now();
    if let Some(ParseDuration(duration)) = opts.duration.take() {
        if let Some(qps) = opts.query_per_second.take() {
            work::work_duration_with_qps(
                || request(client.clone(), url.clone(), tx.clone()),
                qps,
                duration,
                opts.n_workers,
            )
            .await
        } else {
            work::work_duration(
                || request(client.clone(), url.clone(), tx.clone()),
                duration,
                opts.n_workers,
            )
            .await
        }
    } else {
        if let Some(qps) = opts.query_per_second.take() {
            work::work_with_qps(
                || request(client.clone(), url.clone(), tx.clone()),
                qps,
                opts.n_requests,
                opts.n_workers,
            )
            .await
        } else {
            work::work(
                || request(client.clone(), url.clone(), tx.clone()),
                opts.n_requests,
                opts.n_workers,
            )
            .await
        }
    };
    std::mem::drop(tx);

    let res: Vec<_> = realtime_tui.await?;
    let duration = std::time::Instant::now() - start;

    printer::print(&res, duration);

    Ok(())
}
