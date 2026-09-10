use chess_engine::{
    ensure_models, load_network, move_uci, Board, Search, SearchLimits, START_FEN,
};
use std::env;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Instant;

#[derive(Clone)]
struct Config {
    games: usize,
    workers: usize,
    depth: u8,
    movetime: Option<u64>,
    model: String,
    max_plies: usize,
}

fn arg_value(args: &[String], name: &str, default: &str) -> String {
    args.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].clone()).unwrap_or_else(|| default.into())
}

fn play_game(config: &Config, game: usize, training: &Arc<Mutex<std::fs::File>>, pgn_file: &Arc<Mutex<std::fs::File>>) -> (String, usize, u64, u64) {
    let network = load_network(Some(&config.model));
    let mut board = Board::from_fen(START_FEN).expect("start FEN");
    board.set_network(network.clone());
    let mut moves = Vec::new();
    let mut nodes = 0u64;
    let started = Instant::now();
    for _ in 0..config.max_plies {
        if board.is_checkmate() || board.is_stalemate() || board.halfmove >= 100 { break; }
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut search = Search::new(network.clone(), 16, stop);
        let result = search.search(&mut board, SearchLimits {
            depth: config.depth, movetime_ms: config.movetime, ..SearchLimits::default()
        });
        if result.best_move == 0 { break; }
        {
            let mut file = training.lock().unwrap();
            writeln!(file, "{{\"fen\":\"{}\",\"target_cp\":{},\"source\":\"rust-selfplay\"}}", board.fen(), result.score).ok();
        }
        nodes += result.nodes;
        moves.push(result.best_move);
        board.make_move(result.best_move);
    }
    let result = if board.is_checkmate() {
        if board.side == chess_engine::WHITE { "0-1" } else { "1-0" }
    } else { "1/2-1/2" };
    let mut pgn = format!("[Event \"Rust Selfplay\"]\n[Game \"{}\"]\n[Result \"{}\"]\n\n", game, result);
    for (index, mv) in moves.iter().enumerate() {
        if index % 2 == 0 { pgn.push_str(&format!("{}.", index / 2 + 1)); } else { pgn.push_str(&format!("{}...", index / 2 + 1)); }
        pgn.push(' ');
        pgn.push_str(&move_uci(*mv));
    }
    pgn.push_str(&format!(" {}\n\n", result));
    pgn_file.lock().unwrap().write_all(pgn.as_bytes()).ok();
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    (result.into(), moves.len(), nodes, (nodes as f64 / elapsed) as u64)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--help") {
        println!("Selfplay — Rust CPU training data generator");
        println!("--games N --workers N --depth N --movetime MS --model PATH --data DIR --max-plies N");
        return;
    }
    let games = arg_value(&args, "--games", "100").parse().unwrap_or(100);
    let workers = arg_value(&args, "--workers", &std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).to_string()).parse().unwrap_or(1).max(1);
    let depth = arg_value(&args, "--depth", "8").parse().unwrap_or(8);
    let movetime_value = arg_value(&args, "--movetime", "0").parse::<u64>().unwrap_or(0);
    let model = arg_value(&args, "--model", "models/current.nnue");
    let data = PathBuf::from(arg_value(&args, "--data", "data"));
    let max_plies = arg_value(&args, "--max-plies", "300").parse().unwrap_or(300);
    let _ = ensure_models(PathBuf::from(&model).parent().unwrap_or(std::path::Path::new("models")));
    create_dir_all(&data).expect("create data directory");
    let training = Arc::new(Mutex::new(OpenOptions::new().create(true).append(true).open(data.join("training.jsonl")).expect("training data")));
    let pgn_file = Arc::new(Mutex::new(OpenOptions::new().create(true).append(true).open(data.join("games.pgn")).expect("PGN data")));
    let config = Config { games, workers: workers.min(games.max(1)), depth, movetime: (movetime_value > 0).then_some(movetime_value), model, max_plies };
    let (tx, rx) = mpsc::channel();
    for worker in 0..config.workers {
        let tx = tx.clone();
        let training = training.clone();
        let pgn_file = pgn_file.clone();
        let config = config.clone();
        thread::spawn(move || {
            for game in (worker..config.games).step_by(config.workers) {
                let summary = play_game(&config, game + 1, &training, &pgn_file);
                tx.send((game + 1, summary)).ok();
            }
        });
    }
    drop(tx);
    let mut completed = 0;
    let mut white_wins = 0;
    let mut black_wins = 0;
    let mut draws = 0;
    for (game, (result, plies, nodes, nps)) in rx {
        completed += 1;
        match result.as_str() { "1-0" => white_wins += 1, "0-1" => black_wins += 1, _ => draws += 1 }
        println!("game {game} / {} result {result} plies {plies} nodes {nodes} nps {nps}", config.games);
    }
    println!("Selfplay complete: {completed} games · white {white_wins} · draws {draws} · black {black_wins}");
}