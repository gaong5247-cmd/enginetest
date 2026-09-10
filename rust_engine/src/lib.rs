//! Rust rewrite of the dependency-free Python NNUE Alpha chess engine.
//! The core intentionally uses only the Rust standard library.

use std::cmp::{max, min};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::{Duration, Instant};

pub const WHITE: u8 = 0;
pub const BLACK: u8 = 1;
pub const PAWN: u8 = 1;
pub const KNIGHT: u8 = 2;
pub const BISHOP: u8 = 3;
pub const ROOK: u8 = 4;
pub const QUEEN: u8 = 5;
pub const KING: u8 = 6;
pub const EMPTY: u8 = 0;
pub const WK: u8 = 1;
pub const WQ: u8 = 2;
pub const BK: u8 = 4;
pub const BQ: u8 = 8;
pub const START_FEN: &str =
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
pub const INF: i32 = 1_000_000;
pub const MATE: i32 = 100_000;

pub const FLAG_CAPTURE: u8 = 1;
pub const FLAG_EP: u8 = 2;
pub const FLAG_CASTLE: u8 = 4;
pub const FLAG_DOUBLE_PAWN: u8 = 8;
pub const FLAG_PROMOTION: u8 = 16;

const VALUES: [i32; 7] = [0, 100, 320, 330, 500, 900, 20_000];
const PIECE_NAMES: &[u8; 13] = b".PNBRQKpnbrqk";

#[inline]
pub fn bit(sq: usize) -> u64 {
    1u64 << sq
}
#[inline]
pub fn file_of(sq: usize) -> usize {
    sq & 7
}
#[inline]
pub fn rank_of(sq: usize) -> usize {
    sq >> 3
}
#[inline]
pub fn piece_type(piece: u8) -> u8 {
    if piece <= 6 { piece } else { piece - 6 }
}
#[inline]
pub fn piece_color(piece: u8) -> u8 {
    if (1..=6).contains(&piece) { WHITE } else { BLACK }
}
#[inline]
pub fn make_piece(color: u8, kind: u8) -> u8 {
    kind + if color == BLACK { 6 } else { 0 }
}
#[inline]
pub fn pop_lsb(bb: &mut u64) -> usize {
    let sq = bb.trailing_zeros() as usize;
    *bb &= *bb - 1;
    sq
}

pub fn encode_move(from: usize, to: usize, promotion: u8, flags: u8) -> u32 {
    from as u32 | ((to as u32) << 6) | ((promotion as u32) << 12) | ((flags as u32) << 16)
}
#[inline]
pub fn move_from(m: u32) -> usize { (m & 63) as usize }
#[inline]
pub fn move_to(m: u32) -> usize { ((m >> 6) & 63) as usize }
#[inline]
pub fn move_promotion(m: u32) -> u8 { ((m >> 12) & 7) as u8 }
#[inline]
pub fn move_flags(m: u32) -> u8 { (m >> 16) as u8 }
#[inline]
pub fn move_capture(m: u32) -> bool { move_flags(m) & FLAG_CAPTURE != 0 }
pub fn move_uci(m: u32) -> String {
    let mut text = String::with_capacity(5);
    for sq in [move_from(m), move_to(m)] {
        text.push((b'a' + file_of(sq) as u8) as char);
        text.push((b'1' + rank_of(sq) as u8) as char);
    }
    if move_promotion(m) != 0 {
        text.push(match move_promotion(m) {
            KNIGHT => 'n',
            BISHOP => 'b',
            ROOK => 'r',
            _ => 'q',
        });
    }
    text
}

fn step_attacks(sq: usize, deltas: &[(i32, i32)]) -> u64 {
    let file = file_of(sq) as i32;
    let rank = rank_of(sq) as i32;
    let mut attacks = 0;
    for &(df, dr) in deltas {
        let nf = file + df;
        let nr = rank + dr;
        if (0..8).contains(&nf) && (0..8).contains(&nr) {
            attacks |= bit((nr * 8 + nf) as usize);
        }
    }
    attacks
}

fn knight_attacks(sq: usize) -> u64 {
    step_attacks(sq, &[(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)])
}
fn king_attacks(sq: usize) -> u64 {
    step_attacks(sq, &[(1, 1), (1, 0), (1, -1), (0, 1), (0, -1), (-1, 1), (-1, 0), (-1, -1)])
}
fn pawn_attacks(color: u8, sq: usize) -> u64 {
    let rank = rank_of(sq) as i32;
    let file = file_of(sq) as i32;
    let dr = if color == WHITE { 1 } else { -1 };
    let mut attacks = 0;
    for df in [-1, 1] {
        let nf = file + df;
        let nr = rank + dr;
        if (0..8).contains(&nf) && (0..8).contains(&nr) {
            attacks |= bit((nr * 8 + nf) as usize);
        }
    }
    attacks
}
fn rook_attacks(sq: usize, occupancy: u64) -> u64 {
    let mut attacks = 0;
    let f = file_of(sq) as i32;
    let r = rank_of(sq) as i32;
    for nf in (f + 1)..8 {
        let target = bit((r * 8 + nf) as usize);
        attacks |= target;
        if occupancy & target != 0 { break; }
    }
    for nf in (0..f).rev() {
        let target = bit((r * 8 + nf) as usize);
        attacks |= target;
        if occupancy & target != 0 { break; }
    }
    for nr in (r + 1)..8 {
        let target = bit((nr * 8 + f) as usize);
        attacks |= target;
        if occupancy & target != 0 { break; }
    }
    for nr in (0..r).rev() {
        let target = bit((nr * 8 + f) as usize);
        attacks |= target;
        if occupancy & target != 0 { break; }
    }
    attacks
}
fn bishop_attacks(sq: usize, occupancy: u64) -> u64 {
    let mut attacks = 0;
    let f = file_of(sq) as i32;
    let r = rank_of(sq) as i32;
    for &(df, dr) in &[(1, 1), (1, -1), (-1, 1), (-1, -1)] {
        let mut nf = f + df;
        let mut nr = r + dr;
        while (0..8).contains(&nf) && (0..8).contains(&nr) {
            let target = bit((nr * 8 + nf) as usize);
            attacks |= target;
            if occupancy & target != 0 { break; }
            nf += df;
            nr += dr;
        }
    }
    attacks
}
fn queen_attacks(sq: usize, occupancy: u64) -> u64 {
    rook_attacks(sq, occupancy) | bishop_attacks(sq, occupancy)
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}
fn zobrist_piece(piece: u8, sq: usize) -> u64 { splitmix64(0xC0FFEE ^ (piece as u64 * 64 + sq as u64)) }
fn zobrist_castle(castling: u8) -> u64 { splitmix64(0xAA00 ^ castling as u64) }
fn zobrist_ep(ep: i8) -> u64 { splitmix64(0xBB00 ^ (ep as i64 + 1) as u64) }
fn zobrist_side() -> u64 { splitmix64(0xCC00) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move(pub u32);

impl Move {
    pub fn uci(self) -> String { move_uci(self.0) }
}

#[derive(Clone)]
struct BoardState {
    board: [u8; 64],
    bitboards: [u64; 13],
    occupancies: [u64; 3],
    side: u8,
    castling: u8,
    ep: i8,
    halfmove: u16,
    fullmove: u16,
    key: u64,
    accumulator: [i32; 256],
}

#[derive(Clone)]
pub struct Board {
    pub board: [u8; 64],
    pub bitboards: [u64; 13],
    pub occupancies: [u64; 3],
    pub side: u8,
    pub castling: u8,
    pub ep: i8,
    pub halfmove: u16,
    pub fullmove: u16,
    pub key: u64,
    accumulator: [i32; 256],
    network: Arc<NNUE>,
    history: Vec<BoardState>,
}

impl Board {
    pub fn new() -> Self { Self::from_fen(START_FEN).expect("valid start FEN") }

    pub fn from_fen(fen: &str) -> Result<Self, String> {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        if fields.len() != 6 { return Err(format!("invalid FEN: {fen}")); }
        let mut board = [0u8; 64];
        let mut rank: i32 = 7;
        let mut file: usize = 0;
        for ch in fields[0].chars() {
            if ch == '/' { rank -= 1; file = 0; continue; }
            if ch.is_ascii_digit() { file += ch.to_digit(10).unwrap() as usize; continue; }
            let piece = PIECE_NAMES.iter().position(|&v| v as char == ch)
                .ok_or_else(|| format!("invalid piece {ch}"))? as u8;
            if rank < 0 || file >= 8 { return Err("invalid placement".into()); }
            board[rank as usize * 8 + file] = piece;
            file += 1;
        }
        let side = match fields[1] { "w" => WHITE, "b" => BLACK, _ => return Err("invalid side".into()) };
        let castling = if fields[2] == "-" { 0 } else {
            fields[2].bytes().fold(0, |rights, ch| rights | match ch {
                b'K' => WK, b'Q' => WQ, b'k' => BK, b'q' => BQ, _ => 0,
            })
        };
        let ep = if fields[3] == "-" { -1 } else { parse_square(fields[3])? as i8 };
        let halfmove = fields[4].parse::<u16>().map_err(|_| "invalid halfmove")?;
        let fullmove = fields[5].parse::<u16>().map_err(|_| "invalid fullmove")?;
        let network = Arc::new(NNUE::builtin());
        let mut result = Self {
            board, bitboards: [0; 13], occupancies: [0; 3], side, castling, ep,
            halfmove, fullmove, key: 0, accumulator: [0; 256], network, history: Vec::new(),
        };
        result.rebuild();
        Ok(result)
    }

    pub fn set_network(&mut self, network: Arc<NNUE>) {
        self.network = network;
        self.accumulator = self.network.initial_accumulator(&self.board);
    }

    pub fn network(&self) -> Arc<NNUE> { self.network.clone() }

    fn snapshot(&self) -> BoardState {
        BoardState {
            board: self.board, bitboards: self.bitboards, occupancies: self.occupancies,
            side: self.side, castling: self.castling, ep: self.ep, halfmove: self.halfmove,
            fullmove: self.fullmove, key: self.key, accumulator: self.accumulator,
        }
    }
    fn restore(&mut self, state: BoardState) {
        self.board = state.board; self.bitboards = state.bitboards;
        self.occupancies = state.occupancies; self.side = state.side;
        self.castling = state.castling; self.ep = state.ep; self.halfmove = state.halfmove;
        self.fullmove = state.fullmove; self.key = state.key; self.accumulator = state.accumulator;
    }
    fn rebuild(&mut self) {
        self.bitboards = [0; 13];
        for (sq, &piece) in self.board.iter().enumerate() {
            if piece != EMPTY { self.bitboards[piece as usize] |= bit(sq); }
        }
        self.occupancies = [
            (1..=6).map(|p| self.bitboards[p]).fold(0, |a, b| a | b),
            (7..=12).map(|p| self.bitboards[p]).fold(0, |a, b| a | b),
            (1..=12).map(|p| self.bitboards[p]).fold(0, |a, b| a | b),
        ];
        self.key = self.zobrist();
        self.accumulator = self.network.initial_accumulator(&self.board);
    }
    fn rebuild_after(&mut self, before: &[u8; 64]) {
        self.bitboards = [0; 13];
        for (sq, &piece) in self.board.iter().enumerate() {
            if piece != EMPTY { self.bitboards[piece as usize] |= bit(sq); }
        }
        self.occupancies = [
            (1..=6).map(|p| self.bitboards[p]).fold(0, |a, b| a | b),
            (7..=12).map(|p| self.bitboards[p]).fold(0, |a, b| a | b),
            (1..=12).map(|p| self.bitboards[p]).fold(0, |a, b| a | b),
        ];
        self.key = self.zobrist();
        for sq in 0..64 {
            if before[sq] != self.board[sq] {
                if before[sq] != EMPTY { self.network.apply_feature(&mut self.accumulator, before[sq], sq, -1); }
                if self.board[sq] != EMPTY { self.network.apply_feature(&mut self.accumulator, self.board[sq], sq, 1); }
            }
        }
    }
    pub fn zobrist(&self) -> u64 {
        let mut key = zobrist_castle(self.castling) ^ zobrist_ep(self.ep);
        if self.side == BLACK { key ^= zobrist_side(); }
        for (sq, &piece) in self.board.iter().enumerate() {
            if piece != EMPTY { key ^= zobrist_piece(piece, sq); }
        }
        key
    }
    pub fn fen(&self) -> String {
        let mut ranks = Vec::new();
        for rank in (0..8).rev() {
            let mut text = String::new();
            let mut empty = 0;
            for file in 0..8 {
                let piece = self.board[rank * 8 + file];
                if piece == EMPTY { empty += 1; } else {
                    if empty > 0 { text.push(char::from(b'0' + empty)); empty = 0; }
                    text.push(PIECE_NAMES[piece as usize] as char);
                }
            }
            if empty > 0 { text.push(char::from(b'0' + empty)); }
            ranks.push(text);
        }
        let rights = [(b'K', WK), (b'Q', WQ), (b'k', BK), (b'q', BQ)]
            .iter().filter(|(_, flag)| self.castling & flag != 0)
            .map(|(ch, _)| *ch as char).collect::<String>();
        let ep = if self.ep < 0 { "-".to_string() } else { square_name(self.ep as usize) };
        format!("{} {} {} {} {} {}", ranks.join("/"), if self.side == WHITE { "w" } else { "b" },
            if rights.is_empty() { "-".to_string() } else { rights }, ep, self.halfmove, self.fullmove)
    }
    pub fn king_square(&self, color: u8) -> Option<usize> {
        let bb = self.bitboards[make_piece(color, KING) as usize];
        if bb == 0 { None } else { Some(bb.trailing_zeros() as usize) }
    }
    pub fn is_attacked(&self, sq: usize, by: u8) -> bool {
        if pawn_attacks(1 - by, sq) & self.bitboards[make_piece(by, PAWN) as usize] != 0 { return true; }
        if knight_attacks(sq) & self.bitboards[make_piece(by, KNIGHT) as usize] != 0 { return true; }
        if king_attacks(sq) & self.bitboards[make_piece(by, KING) as usize] != 0 { return true; }
        let occ = self.occupancies[2];
        if rook_attacks(sq, occ) & (self.bitboards[make_piece(by, ROOK) as usize] | self.bitboards[make_piece(by, QUEEN) as usize]) != 0 { return true; }
        bishop_attacks(sq, occ) & (self.bitboards[make_piece(by, BISHOP) as usize] | self.bitboards[make_piece(by, QUEEN) as usize]) != 0
    }
    pub fn in_check(&self, color: u8) -> bool {
        self.king_square(color).map(|sq| self.is_attacked(sq, 1 - color)).unwrap_or(true)
    }
    fn push_piece_moves(&self, moves: &mut Vec<u32>, from: usize, mut targets: u64, captures_only: bool) {
        let own = self.occupancies[self.side as usize];
        let enemy = self.occupancies[(1 - self.side) as usize];
        targets &= !own;
        if captures_only { targets &= enemy; }
        while targets != 0 {
            let to = pop_lsb(&mut targets);
            let capture = if self.board[to] != EMPTY { FLAG_CAPTURE } else { 0 };
            moves.push(encode_move(from, to, 0, capture));
        }
    }
    fn push_pawn_move(moves: &mut Vec<u32>, from: usize, to: usize, flags: u8) {
        if rank_of(to) == 0 || rank_of(to) == 7 {
            for promotion in [QUEEN, ROOK, BISHOP, KNIGHT] {
                moves.push(encode_move(from, to, promotion, flags | FLAG_PROMOTION));
            }
        } else { moves.push(encode_move(from, to, 0, flags)); }
    }
    fn castling_moves(&self, moves: &mut Vec<u32>) {
        if self.side == WHITE {
            if self.castling & WK != 0 && self.board[5] == 0 && self.board[6] == 0 && self.board[7] == make_piece(WHITE, ROOK)
                && !self.in_check(WHITE) && !self.is_attacked(5, BLACK) && !self.is_attacked(6, BLACK) { moves.push(encode_move(4, 6, 0, FLAG_CASTLE)); }
            if self.castling & WQ != 0 && self.board[1] == 0 && self.board[2] == 0 && self.board[3] == 0 && self.board[0] == make_piece(WHITE, ROOK)
                && !self.in_check(WHITE) && !self.is_attacked(3, BLACK) && !self.is_attacked(2, BLACK) { moves.push(encode_move(4, 2, 0, FLAG_CASTLE)); }
        } else {
            if self.castling & BK != 0 && self.board[61] == 0 && self.board[62] == 0 && self.board[63] == make_piece(BLACK, ROOK)
                && !self.in_check(BLACK) && !self.is_attacked(61, WHITE) && !self.is_attacked(62, WHITE) { moves.push(encode_move(60, 62, 0, FLAG_CASTLE)); }
            if self.castling & BQ != 0 && self.board[57] == 0 && self.board[58] == 0 && self.board[59] == 0 && self.board[56] == make_piece(BLACK, ROOK)
                && !self.in_check(BLACK) && !self.is_attacked(59, WHITE) && !self.is_attacked(58, WHITE) { moves.push(encode_move(60, 58, 0, FLAG_CASTLE)); }
        }
    }
    pub fn pseudo_moves(&self, captures_only: bool) -> Vec<u32> {
        let own = self.occupancies[self.side as usize];
        let enemy = self.occupancies[(1 - self.side) as usize];
        let occ = self.occupancies[2];
        let mut moves = Vec::new();
        for from in 0..64 {
            let piece = self.board[from];
            if piece == 0 || piece_color(piece) != self.side { continue; }
            match piece_type(piece) {
                PAWN => {
                    let direction: i32 = if self.side == WHITE { 8 } else { -8 };
                    let start_rank = if self.side == WHITE { 1 } else { 6 };
                    let one = from as i32 + direction;
                    if !captures_only && (0..64).contains(&one) && self.board[one as usize] == 0 {
                        Self::push_pawn_move(&mut moves, from, one as usize, 0);
                        let two = from as i32 + direction * 2;
                        if rank_of(from) == start_rank && self.board[two as usize] == 0 {
                            moves.push(encode_move(from, two as usize, 0, FLAG_DOUBLE_PAWN));
                        }
                    }
                    let mut targets = pawn_attacks(self.side, from) & enemy;
                    if self.ep >= 0 { targets |= pawn_attacks(self.side, from) & bit(self.ep as usize); }
                    while targets != 0 {
                        let to = pop_lsb(&mut targets);
                        Self::push_pawn_move(&mut moves, from, to, if to as i8 == self.ep { FLAG_CAPTURE | FLAG_EP } else { FLAG_CAPTURE });
                    }
                }
                KNIGHT => self.push_piece_moves(&mut moves, from, knight_attacks(from), captures_only),
                BISHOP => self.push_piece_moves(&mut moves, from, bishop_attacks(from, occ), captures_only),
                ROOK => self.push_piece_moves(&mut moves, from, rook_attacks(from, occ), captures_only),
                QUEEN => self.push_piece_moves(&mut moves, from, queen_attacks(from, occ), captures_only),
                KING => {
                    self.push_piece_moves(&mut moves, from, king_attacks(from), captures_only);
                    if !captures_only { self.castling_moves(&mut moves); }
                }
                _ => {}
            }
        }
        moves
    }
    pub fn legal_moves(&mut self) -> Vec<u32> { self.legal_moves_filtered(false) }
    pub fn legal_captures(&mut self) -> Vec<u32> { self.legal_moves_filtered(true) }
    fn legal_moves_filtered(&mut self, captures_only: bool) -> Vec<u32> {
        let pseudo = self.pseudo_moves(captures_only);
        let mut legal = Vec::with_capacity(pseudo.len());
        for mv in pseudo {
            self.make_move(mv);
            if !self.in_check(1 - self.side) { legal.push(mv); }
            self.undo_move();
        }
        legal
    }
    pub fn make_move(&mut self, mv: u32) {
        self.history.push(self.snapshot());
        let from = move_from(mv);
        let to = move_to(mv);
        let moving = self.board[from];
        let mut captured = self.board[to];
        let flags = move_flags(mv);
        let before = self.board;
        self.board[from] = 0;
        if flags & FLAG_EP != 0 {
            let capture_sq = if self.side == WHITE { to - 8 } else { to + 8 };
            captured = self.board[capture_sq];
            self.board[capture_sq] = 0;
        }
        self.board[to] = if move_promotion(mv) != 0 { make_piece(self.side, move_promotion(mv)) } else { moving };
        if flags & FLAG_CASTLE != 0 {
            let (rook_from, rook_to) = if to > from { (from + 3, from + 1) } else { (from - 4, from - 1) };
            self.board[rook_to] = self.board[rook_from];
            self.board[rook_from] = 0;
        }
        self.update_castling(from, to, moving, captured);
        self.ep = if flags & FLAG_DOUBLE_PAWN != 0 { (from as i8 + to as i8) / 2 } else { -1 };
        self.halfmove = if piece_type(moving) == PAWN || captured != 0 { 0 } else { self.halfmove + 1 };
        if self.side == BLACK { self.fullmove += 1; }
        self.side = 1 - self.side;
        self.rebuild_after(&before);
    }
    fn update_castling(&mut self, from: usize, to: usize, moving: u8, captured: u8) {
        if moving == make_piece(WHITE, KING) { self.castling &= !(WK | WQ); }
        if moving == make_piece(BLACK, KING) { self.castling &= !(BK | BQ); }
        if moving == make_piece(WHITE, ROOK) { if from == 0 { self.castling &= !WQ; } if from == 7 { self.castling &= !WK; } }
        if moving == make_piece(BLACK, ROOK) { if from == 56 { self.castling &= !BQ; } if from == 63 { self.castling &= !BK; } }
        if captured == make_piece(WHITE, ROOK) { if to == 0 { self.castling &= !WQ; } if to == 7 { self.castling &= !WK; } }
        if captured == make_piece(BLACK, ROOK) { if to == 56 { self.castling &= !BQ; } if to == 63 { self.castling &= !BK; } }
    }
    pub fn undo_move(&mut self) {
        if let Some(state) = self.history.pop() { self.restore(state); }
    }
    pub fn find_move(&mut self, text: &str) -> Result<u32, String> {
        self.legal_moves().into_iter().find(|&mv| move_uci(mv) == text)
            .ok_or_else(|| format!("illegal move {text} in {}", self.fen()))
    }
    pub fn is_checkmate(&mut self) -> bool { self.in_check(self.side) && self.legal_moves().is_empty() }
    pub fn is_stalemate(&mut self) -> bool { !self.in_check(self.side) && self.legal_moves().is_empty() }
    pub fn material(&self) -> i32 {
        self.board.iter().filter(|&&p| p != 0).map(|&p| {
            let value = VALUES[piece_type(p) as usize];
            if piece_color(p) == WHITE { value } else { -value }
        }).sum()
    }
}

pub fn parse_square(text: &str) -> Result<usize, String> {
    let bytes = text.as_bytes();
    if bytes.len() != 2 || !(b'a'..=b'h').contains(&bytes[0]) || !(b'1'..=b'8').contains(&bytes[1]) {
        return Err(format!("invalid square {text}"));
    }
    Ok((bytes[1] - b'1') as usize * 8 + (bytes[0] - b'a') as usize)
}
pub fn square_name(sq: usize) -> String {
    format!("{}{}", (b'a' + file_of(sq) as u8) as char, (b'1' + rank_of(sq) as u8) as char)
}

pub const NNUE_INPUTS: usize = 768;
pub const NNUE_HIDDEN: usize = 256;
pub const NNUE_OUTPUT_HIDDEN: usize = 64;
const NNUE_MAGIC: &[u8; 8] = b"PYNNUE01";

#[derive(Clone)]
pub struct NNUE {
    pub input_weights: Vec<i16>,
    hidden_weights: Vec<i16>,
    hidden_bias: Vec<i16>,
    output_weights: Vec<i16>,
    output_bias: i32,
    pub name: String,
}

fn seeded_values(count: usize, mut seed: u32, scale: i16) -> Vec<i16> {
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        result.push((((seed >> 24) % (2 * scale as u32 + 1)) as i16) - scale);
    }
    result
}

impl NNUE {
    pub fn builtin() -> Self {
        Self {
            input_weights: seeded_values(NNUE_INPUTS * NNUE_HIDDEN, 0x9E3779B9, 4),
            hidden_weights: seeded_values(NNUE_HIDDEN * NNUE_OUTPUT_HIDDEN, 0x243F6A88, 3),
            hidden_bias: vec![0; NNUE_HIDDEN],
            output_weights: seeded_values(NNUE_OUTPUT_HIDDEN, 0xB7E15162, 12),
            output_bias: 0,
            name: "builtin".into(),
        }
    }
    pub fn initial_accumulator(&self, board: &[u8; 64]) -> [i32; 256] {
        let mut accumulator = [0i32; NNUE_HIDDEN];
        for (index, &value) in self.hidden_bias.iter().enumerate() { accumulator[index] = value as i32; }
        for (sq, &piece) in board.iter().enumerate() {
            if piece != EMPTY { self.apply_feature(&mut accumulator, piece, sq, 1); }
        }
        accumulator
    }
    pub fn apply_feature(&self, accumulator: &mut [i32; 256], piece: u8, sq: usize, sign: i32) {
        let offset = ((piece - 1) as usize * 64 + sq) * NNUE_HIDDEN;
        for hidden in 0..NNUE_HIDDEN {
            accumulator[hidden] += sign * self.input_weights[offset + hidden] as i32;
        }
    }
    #[inline]
    fn crelu(value: i32) -> i32 { value.clamp(0, 127) }
    pub fn evaluate_white(&self, board: &Board) -> i32 {
        let mut result = self.output_bias;
        for out in 0..NNUE_OUTPUT_HIDDEN {
            let base = out * NNUE_HIDDEN;
            let mut total = 0i32;
            for hidden in 0..NNUE_HIDDEN {
                total += Self::crelu(board.accumulator[hidden]) * self.hidden_weights[base + hidden] as i32;
            }
            result += Self::crelu(total / 32 + self.output_weights[out] as i32);
        }
        result / 8 + board.material()
    }
    pub fn evaluate(&self, board: &Board) -> i32 {
        let score = self.evaluate_white(board);
        if board.side == WHITE { score } else { -score }
    }
    pub fn verify_accumulator(&self, board: &Board) -> bool {
        let expected = self.initial_accumulator(&board.board);
        expected == board.accumulator
    }
    fn read_u32(data: &[u8], offset: &mut usize) -> Result<u32, String> {
        if *offset + 4 > data.len() { return Err("truncated model".into()); }
        let value = u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap());
        *offset += 4;
        Ok(value)
    }
    fn read_i16_vec(data: &[u8], offset: &mut usize, count: usize) -> Result<Vec<i16>, String> {
        if *offset + count * 2 > data.len() { return Err("truncated model weights".into()); }
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            result.push(i16::from_le_bytes(data[*offset..*offset + 2].try_into().unwrap()));
            *offset += 2;
        }
        Ok(result)
    }
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let data = fs::read(path.as_ref()).map_err(|e| e.to_string())?;
        if data.len() < 36 || &data[..8] != NNUE_MAGIC { return Err("unsupported NNUE model".into()); }
        let mut offset = 8;
        let version = Self::read_u32(&data, &mut offset)?;
        if version != 1 { return Err("unsupported NNUE model version".into()); }
        let name_len = Self::read_u32(&data, &mut offset)? as usize;
        if offset + name_len > data.len() { return Err("truncated model name".into()); }
        let name = String::from_utf8_lossy(&data[offset..offset + name_len]).to_string();
        offset += name_len;
        let lengths: Vec<usize> = (0..5).map(|_| Self::read_u32(&data, &mut offset).map(|v| v as usize)).collect::<Result<_, _>>()?;
        if lengths != [NNUE_INPUTS * NNUE_HIDDEN, NNUE_HIDDEN * NNUE_OUTPUT_HIDDEN, NNUE_HIDDEN, NNUE_OUTPUT_HIDDEN, 1] {
            return Err("NNUE dimensions do not match".into());
        }
        let input_weights = Self::read_i16_vec(&data, &mut offset, lengths[0])?;
        let hidden_weights = Self::read_i16_vec(&data, &mut offset, lengths[1])?;
        let hidden_bias = Self::read_i16_vec(&data, &mut offset, lengths[2])?;
        let output_weights = Self::read_i16_vec(&data, &mut offset, lengths[3])?;
        if offset + 4 > data.len() { return Err("truncated output bias".into()); }
        let output_bias = i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        Ok(Self { input_weights, hidden_weights, hidden_bias, output_weights, output_bias, name })
    }
    pub fn save(&self, path: impl AsRef<Path>, name: &str) -> Result<(), String> {
        let mut data = Vec::new();
        data.extend_from_slice(NNUE_MAGIC);
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&(name.len() as u32).to_le_bytes());
        data.extend_from_slice(name.as_bytes());
        let lengths = [self.input_weights.len(), self.hidden_weights.len(), self.hidden_bias.len(), self.output_weights.len(), 1];
        for length in lengths { data.extend_from_slice(&(length as u32).to_le_bytes()); }
        for value in self.input_weights.iter().chain(self.hidden_weights.iter()).chain(self.hidden_bias.iter()).chain(self.output_weights.iter()) {
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.extend_from_slice(&self.output_bias.to_le_bytes());
        if let Some(parent) = path.as_ref().parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        fs::write(path, data).map_err(|e| e.to_string())
    }
}

pub fn load_network(path: Option<&str>) -> Arc<NNUE> {
    path.and_then(|p| NNUE::load(p).ok()).map(|m| Arc::new(m)).unwrap_or_else(|| Arc::new(NNUE::builtin()))
}

#[derive(Clone, Copy)]
pub struct SearchLimits {
    pub depth: u8,
    pub movetime_ms: Option<u64>,
    pub wtime_ms: Option<u64>,
    pub btime_ms: Option<u64>,
    pub winc_ms: u64,
    pub binc_ms: u64,
}
impl Default for SearchLimits {
    fn default() -> Self {
        Self { depth: 64, movetime_ms: None, wtime_ms: None, btime_ms: None, winc_ms: 0, binc_ms: 0 }
    }
}

#[derive(Clone)]
struct TTEntry { depth: i32, score: i32, bound: u8, best: u32 }
const EXACT: u8 = 0;
const LOWER: u8 = 1;
const UPPER: u8 = 2;

pub struct SearchResult {
    pub best_move: u32,
    pub score: i32,
    pub depth: u8,
    pub seldepth: u8,
    pub nodes: u64,
    pub time_ms: u64,
    pub pv: Vec<u32>,
}
impl SearchResult {
    pub fn nps(&self) -> u64 { self.nodes.saturating_mul(1000) / self.time_ms.max(1) }
}

pub struct Search {
    pub network: Arc<NNUE>,
    tt: HashMap<u64, TTEntry>,
    hash_limit: usize,
    killers: [[u32; 2]; 128],
    history: [[i32; 64]; 13],
    pub nodes: u64,
    seldepth: u8,
    deadline: Instant,
    stop: Arc<AtomicBool>,
}

impl Search {
    pub fn new(network: Arc<NNUE>, hash_mb: usize, stop: Arc<AtomicBool>) -> Self {
        Self {
            network, tt: HashMap::new(), hash_limit: (hash_mb.max(1) * 16_384).max(1024),
            killers: [[0; 2]; 128], history: [[0; 64]; 13], nodes: 0, seldepth: 0,
            deadline: Instant::now(), stop,
        }
    }
    fn should_stop(&self) -> bool { self.stop.load(Ordering::Relaxed) || Instant::now() >= self.deadline }
    pub fn search(&mut self, board: &mut Board, limits: SearchLimits) -> SearchResult {
        board.set_network(self.network.clone());
        self.nodes = 0; self.seldepth = 0;
        let started = Instant::now();
        let budget = limits.movetime_ms.or_else(|| {
            let remaining = if board.side == WHITE { limits.wtime_ms } else { limits.btime_ms };
            remaining.map(|time| time / 25 + if board.side == WHITE { limits.winc_ms } else { limits.binc_ms } / 2)
        });
        self.deadline = budget.map(|ms| started + Duration::from_millis(ms.max(10))).unwrap_or_else(|| started + Duration::from_secs(86_400));
        let mut root_moves = board.legal_moves();
        if root_moves.is_empty() {
            return SearchResult { best_move: 0, score: 0, depth: 0, seldepth: 0, nodes: 0, time_ms: 0, pv: vec![] };
        }
        let mut best_move = root_moves[0];
        let mut best_score = 0;
        let mut best_pv = vec![best_move];
        let mut completed = 0;
        for depth in 1..=limits.depth.max(1) {
            if self.should_stop() { break; }
            let (score, pv) = if completed >= 2 {
                let alpha = max(-INF, best_score - 50);
                let beta = min(INF, best_score + 50);
                let narrow = self.root(board, depth as i32, &root_moves, alpha, beta);
                if !self.should_stop() && (narrow.0 <= alpha || narrow.0 >= beta) {
                    self.root(board, depth as i32, &root_moves, -INF, INF)
                } else { narrow }
            } else { self.root(board, depth as i32, &root_moves, -INF, INF) };
            if self.should_stop() && completed > 0 { break; }
            if let Some(&move_) = pv.first() {
                best_move = move_; best_score = score; best_pv = pv.clone();
                root_moves.sort_by_key(|m| if pv.contains(m) { 0 } else { 1 });
            }
            completed = depth;
        }
        SearchResult {
            best_move, score: best_score, depth: completed, seldepth: self.seldepth,
            nodes: self.nodes, time_ms: started.elapsed().as_millis() as u64, pv: best_pv,
        }
    }
    fn root(&mut self, board: &mut Board, depth: i32, root_moves: &[u32], mut alpha: i32, beta: i32) -> (i32, Vec<u32>) {
        let mut best_score = -INF;
        let mut best_pv = Vec::new();
        for (index, &mv) in root_moves.iter().enumerate() {
            if self.should_stop() { break; }
            board.make_move(mv);
            let (child, pv) = self.negamax(board, depth - 1, -beta, -alpha, 1);
            board.undo_move();
            let score = -child;
            if score > best_score { best_score = score; best_pv = std::iter::once(mv).chain(pv).collect(); }
            if score > alpha { alpha = score; }
            if alpha >= beta { break; }
            let _ = index;
        }
        (best_score, best_pv)
    }
    fn negamax(&mut self, board: &mut Board, depth: i32, mut alpha: i32, mut beta: i32, ply: usize) -> (i32, Vec<u32>) {
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply as u8);
        if self.should_stop() { return (0, vec![]); }
        if board.is_checkmate() { return (-MATE + ply as i32, vec![]); }
        if board.is_stalemate() || board.halfmove >= 100 { return (0, vec![]); }
        if depth <= 0 { return self.quiescence(board, alpha, beta, ply); }
        let original_alpha = alpha;
        let mut tt_move = 0;
        if let Some(entry) = self.tt.get(&board.key).cloned() {
            tt_move = entry.best;
            if entry.depth >= depth {
                if entry.bound == EXACT { return (entry.score, if entry.best == 0 { vec![] } else { vec![entry.best] }); }
                if entry.bound == LOWER { alpha = max(alpha, entry.score); }
                if entry.bound == UPPER { beta = min(beta, entry.score); }
                if alpha >= beta { return (entry.score, if entry.best == 0 { vec![] } else { vec![entry.best] }); }
            }
        }
        let in_check = board.in_check(board.side);
        if depth >= 3 && !in_check && self.has_non_pawn_material(board, board.side) {
            let old_side = board.side;
            board.side = 1 - board.side;
            board.key = board.zobrist();
            let (score, _) = self.negamax(board, depth - 3, -beta, -beta + 1, ply + 1);
            board.side = old_side;
            board.key = board.zobrist();
            if -score >= beta { return (beta, vec![]); }
        }
        let mut moves = board.legal_moves();
        if moves.is_empty() { return (if in_check { -MATE + ply as i32 } else { 0 }, vec![]); }
        moves.sort_by_key(|&mv| -(self.move_score(board, mv, tt_move, ply) as i64));
        let mut best_score = -INF;
        let mut best_move = 0;
        let mut best_pv = vec![];
        for (index, mv) in moves.iter().copied().enumerate() {
            if self.should_stop() { break; }
            let quiet = !move_capture(mv) && move_promotion(mv) == 0;
            board.make_move(mv);
            let extension = if board.in_check(board.side) && depth >= 2 { 1 } else { 0 };
            let reduction = if depth >= 3 && index >= 4 && quiet && !in_check { 1 } else { 0 };
            let score;
            let pv;
            if index == 0 {
                let (child, child_pv) = self.negamax(board, depth - 1 + extension, -beta, -alpha, ply + 1);
                score = -child; pv = child_pv;
            } else {
                let (child, child_pv) = self.negamax(board, depth - 1 - reduction + extension, -alpha - 1, -alpha, ply + 1);
                let mut value = -child;
                let mut line = child_pv;
                if value > alpha && value < beta {
                    let (re_child, re_pv) = self.negamax(board, depth - 1 + extension, -beta, -alpha, ply + 1);
                    value = -re_child; line = re_pv;
                }
                score = value; pv = line;
            }
            board.undo_move();
            if score > best_score { best_score = score; best_move = mv; best_pv = std::iter::once(mv).chain(pv).collect(); }
            if score > alpha { alpha = score; }
            if alpha >= beta {
                if quiet {
                    if self.killers[ply][0] != mv { self.killers[ply][1] = self.killers[ply][0]; self.killers[ply][0] = mv; }
                    self.history[board.board[move_from(mv)] as usize][move_to(mv)] += depth * depth;
                }
                break;
            }
        }
        let bound = if best_score <= original_alpha { UPPER } else if best_score >= beta { LOWER } else { EXACT };
        if self.tt.len() >= self.hash_limit && !self.tt.contains_key(&board.key) {
            if let Some(key) = self.tt.keys().next().copied() { self.tt.remove(&key); }
        }
        self.tt.insert(board.key, TTEntry { depth, score: best_score, bound, best: best_move });
        (best_score, best_pv)
    }
    fn quiescence(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: usize) -> (i32, Vec<u32>) {
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply as u8);
        let stand = self.network.evaluate(board);
        if stand >= beta { return (beta, vec![]); }
        if stand > alpha { alpha = stand; }
        let mut best_pv = vec![];
        let mut captures = board.legal_captures();
        captures.sort_by_key(|&mv| -(self.move_score(board, mv, 0, ply) as i64));
        for mv in captures {
            if self.should_stop() { break; }
            board.make_move(mv);
            let (child, pv) = self.quiescence(board, -beta, -alpha, ply + 1);
            board.undo_move();
            let score = -child;
            if score >= beta { return (beta, std::iter::once(mv).chain(pv).collect()); }
            if score > alpha { alpha = score; best_pv = std::iter::once(mv).chain(pv).collect(); }
        }
        (alpha, best_pv)
    }
    fn move_score(&self, board: &Board, mv: u32, tt_move: u32, ply: usize) -> i32 {
        if mv == tt_move { return 10_000_000; }
        let moving = board.board[move_from(mv)];
        if move_capture(mv) {
            let captured = board.board[move_to(mv)];
            return 1_000_000 + VALUES[piece_type(if captured == 0 { PAWN } else { captured }) as usize] * 10 - VALUES[piece_type(moving) as usize];
        }
        if mv == self.killers[ply.min(127)][0] { return 900_000; }
        if mv == self.killers[ply.min(127)][1] { return 800_000; }
        self.history[moving as usize][move_to(mv)]
    }
    fn has_non_pawn_material(&self, board: &Board, color: u8) -> bool {
        [KNIGHT, BISHOP, ROOK, QUEEN].iter().any(|&p| board.bitboards[make_piece(color, p) as usize] != 0)
    }
}

pub fn format_info(result: &SearchResult, hashfull: usize) -> String {
    let score = if result.score.abs() > MATE - 1000 {
        format!("mate {}", (MATE - result.score.abs() + 1) / 2)
    } else { format!("cp {}", result.score) };
    let pv = result.pv.iter().map(|&mv| move_uci(mv)).collect::<Vec<_>>().join(" ");
    format!("info depth {} seldepth {} score {} nodes {} nps {} time {} hashfull {} pv {}",
        result.depth, result.seldepth, score, result.nodes, result.nps(), result.time_ms, hashfull, pv)
}

fn parse_limits(args: &[&str]) -> SearchLimits {
    let mut limits = SearchLimits::default();
    let mut index = 0;
    while index + 1 < args.len() {
        if let Ok(value) = args[index + 1].parse::<u64>() {
            match args[index] {
                "depth" => limits.depth = value as u8,
                "movetime" => limits.movetime_ms = Some(value),
                "wtime" => limits.wtime_ms = Some(value),
                "btime" => limits.btime_ms = Some(value),
                "winc" => limits.winc_ms = value,
                "binc" => limits.binc_ms = value,
                _ => {}
            }
        }
        index += 2;
    }
    limits
}

pub fn run_uci() {
    let (command_tx, command_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines().flatten() {
            if command_tx.send(line).is_err() { break; }
        }
    });
    let mut board = Board::new();
    let mut model_path: Option<String> = None;
    let mut hash_mb = 32usize;
    let mut stop_handle: Option<Arc<AtomicBool>> = None;
    let mut running = true;
    while running {
        match command_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(line) => {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.is_empty() { continue; }
                match parts[0].to_ascii_lowercase().as_str() {
                    "uci" => {
                        println!("id name Python NNUE Alpha Rust");
                        println!("id author Replit");
                        println!("option name Hash type spin default 32 min 1 max 1024");
                        println!("option name ModelFile type string default");
                        println!("uciok");
                    }
                    "isready" => println!("readyok"),
                    "setoption" => {
                        if let Some(name_index) = parts.iter().position(|p| *p == "name") {
                            let value_index = parts.iter().position(|p| *p == "value");
                            let end = value_index.unwrap_or(parts.len());
                            let option = parts[name_index + 1..end].join(" ").to_ascii_lowercase();
                            let value = value_index.map(|i| parts[i + 1..].join("")).unwrap_or_default();
                            if option == "hash" { hash_mb = value.parse::<usize>().unwrap_or(32).clamp(1, 1024); }
                            if option == "modelfile" { model_path = if value.is_empty() { None } else { Some(value) }; }
                        }
                    }
                    "ucinewgame" => { if let Some(stop) = &stop_handle { stop.store(true, Ordering::Relaxed); } board = Board::new(); }
                    "position" => {
                        if let Some(stop) = &stop_handle { stop.store(true, Ordering::Relaxed); }
                        if parts.len() >= 2 {
                            let mut move_start = parts.len();
                            if parts[1] == "startpos" {
                                board = Board::new();
                                if parts.get(2) == Some(&"moves") { move_start = 3; }
                            } else if parts[1] == "fen" {
                                move_start = parts.iter().position(|p| *p == "moves").unwrap_or(parts.len());
                                let fen = parts[2..move_start].join(" ");
                                if let Ok(parsed) = Board::from_fen(&fen) { board = parsed; }
                                if move_start < parts.len() { move_start += 1; }
                            }
                            for text in &parts[move_start..] {
                                if let Ok(mv) = board.find_move(text) { board.make_move(mv); }
                            }
                        }
                    }
                    "go" => {
                        if let Some(stop) = &stop_handle { stop.store(true, Ordering::Relaxed); }
                        let limits = parse_limits(&parts[1..]);
                        let mut search_board = board.clone();
                        let stop = Arc::new(AtomicBool::new(false));
                        stop_handle = Some(stop.clone());
                        let network = load_network(model_path.as_deref());
                        thread::spawn(move || {
                            let mut search = Search::new(network, hash_mb, stop);
                            let result = search.search(&mut search_board, limits);
                            println!("{}", format_info(&result, search.tt.len() * 1000 / search.hash_limit.max(1)));
                            println!("bestmove {}", move_uci(result.best_move));
                        });
                    }
                    "stop" => { if let Some(stop) = &stop_handle { stop.store(true, Ordering::Relaxed); } }
                    "quit" => { if let Some(stop) = &stop_handle { stop.store(true, Ordering::Relaxed); } running = false; }
                    _ => {}
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

pub fn perft(board: &mut Board, depth: u8) -> u64 {
    if depth == 0 { return 1; }
    let moves = board.legal_moves();
    let mut total = 0;
    for mv in moves {
        board.make_move(mv);
        total += perft(board, depth - 1);
        board.undo_move();
    }
    total
}

pub fn ensure_models(dir: impl AsRef<Path>) -> Result<(PathBuf, PathBuf), String> {
    let dir = dir.as_ref();
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let current = dir.join("current.nnue");
    let previous = dir.join("previous.nnue");
    if !current.exists() { NNUE::builtin().save(&current, "current")?; }
    if !previous.exists() { NNUE::builtin().save(&previous, "previous")?; }
    Ok((current, previous))
}
