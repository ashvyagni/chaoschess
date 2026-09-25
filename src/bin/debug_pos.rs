use chess::{Board, BoardStatus, MoveGen};
use std::str::FromStr;
fn main() {
    let fen = std::env::args().nth(1).expect("fen");
    let board = Board::from_str(&fen).expect("valid fen");
    println!("{board}\n");
    println!("side to move: {:?}", board.side_to_move());
    println!("status: {:?}", board.status());
    println!("in check: {}", board.checkers() != &chess::EMPTY);
    println!("legal moves ({}):", MoveGen::new_legal(&board).count());
    for m in MoveGen::new_legal(&board) {
        let child = board.make_move_new(m);
        print!("  {m} -> {:?}", child.status());
        if child.status() == BoardStatus::Checkmate {
            print!("   *** MATE ***");
        }
        println!();
    }
}
