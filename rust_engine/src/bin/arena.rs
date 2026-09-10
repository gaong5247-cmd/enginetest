use chess_engine::{ensure_models, load_network, Board, Search, SearchLimits, WHITE};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

fn arg_value(args: &[String], name: &str, default: &str) -> String {
    args.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].clone()).unwrap_or_else(|| default.into())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--help") {
        println!("Arena — Rust current vs previous model match");
        println!("--games N --depth N --movetime MS --current PATH --previous PATH --promote-score PERCENT --promote-min-games N");
        return;
    }
    let games = arg_value(&args, "--games", "50").parse::<usize>().unwrap_or(50);
    let depth = arg_value(&args, "--depth", "8").parse::<u8>().unwrap_or(8);
    let movetime_value = arg_value(&args, "--movetime", "0").parse::<u64>().unwrap_or(0);
    let current_path = PathBuf::from(arg_value(&args, "--current", "models/current.nnue"));
    let previous_path = PathBuf::from(arg_value(&args, "--previous", "models/previous.nnue"));
    let pass_score = arg_value(&args, "--promote-score", "55").parse::<f64>().unwrap_or(55.0);
    let min_games = arg_value(&args, "--promote-min-games", "100").parse::<usize>().unwrap_or(100);
    let _ = ensure_models(current_path.parent().unwrap_or(std::path::Path::new("models")));
    let current = load_network(current_path.to_str());
    let previous = load_network(previous_path.to_str());
    let mut current_wins = 0;
    let mut previous_wins = 0;
    let mut draws = 0;
    let mut total_plies = 0usize;
    for game in 0..games {
        let current_color = if game % 2 == 0 { WHITE } else { 1 - WHITE };
        let mut board = Board::new();
        let mut plies = 0;
        for _ in 0..300 {
            if board.is_checkmate() || board.is_stalemate() || board.halfmove >= 100 { break; }
            let network = if board.side == current_color { current.clone() } else { previous.clone() };
            board.set_network(network.clone());
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let mut search = Search::new(network, 16, stop);
            let result = search.search(&mut board, SearchLimits { depth, movetime_ms: (movetime_value > 0).then_some(movetime_value), ..SearchLimits::default() });
            if result.best_move == 0 { break; }
            board.make_move(result.best_move);
            plies += 1;
        }
        let winner = if board.is_checkmate() { Some(1 - board.side) } else { None };
        match winner {
            Some(color) if color == current_color => current_wins += 1,
            Some(_) => previous_wins += 1,
            None => draws += 1,
        }
        total_plies += plies;
        let score = (current_wins as f64 + draws as f64 / 2.0) / (game + 1) as f64 * 100.0;
        println!("game {} / {} · current {} · previous {} · draws {} · score {:.1}%", game + 1, games, current_wins, previous_wins, draws, score);
    }
    let score = (current_wins as f64 + draws as f64 / 2.0) / games.max(1) as f64 * 100.0;
    println!("\nCURRENT MODEL\nScore: {:.1}%\n\nPREVIOUS MODEL\nScore: {:.1}%\n\nRESULT: {}", score, 100.0 - score, if score >= pass_score && games >= min_games { "CURRENT MODEL IS STRONGER" } else { "CURRENT MODEL DID NOT PASS" });
    println!("Average game length: {:.1} plies", total_plies as f64 / games.max(1) as f64);
    if score >= pass_score && games >= min_games {
        if let Err(error) = fs::copy(&current_path, &previous_path) { eprintln!("promotion failed: {error}"); }
        else { println!("Promoted current model to previous checkpoint."); }
    }
}