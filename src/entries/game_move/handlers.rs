use std::collections::HashMap;

use hdk::prelude::*;
use hdk::prelude::holo_hash::EntryHashB64;

use crate::{
    prelude::{GameEntry, MoveInfo},
    signal::{self, SignalPayload},
};

use super::GameMoveEntry;

/** Public handlers */

/**
 * Creates the next move for the given game, linking the game to the move
 * If this is the first move, we should
 */
pub fn create_move<M>(
    game_hash: EntryHashB64,
    previous_move_hash: Option<EntryHashB64>,
    game_move: M,
) -> ExternResult<EntryHashB64>
where
    M: TryFrom<SerializedBytes> + TryInto<SerializedBytes>,
{
    let move_bytes: SerializedBytes = game_move
        .try_into()
        .or(Err(WasmError::Guest("Coulnd't serialize game move".into())))?;

    let game_move = GameMoveEntry {
        game_hash: game_hash.clone().into(),
        author_pub_key: agent_info()?.agent_latest_pubkey.into(),
        game_move: move_bytes,
        previous_move_hash: previous_move_hash.clone(),
    };

    create_entry(&game_move)?;

    let move_hash = hash_entry(&game_move)?;

    create_link(
        EntryHash::from(game_hash.clone()),
        move_hash.clone(),
        game_to_move_tag(),
    )?;

    // Sends the newly created move to all opponents of the game

    let element = get(EntryHash::from(game_hash), GetOptions::default())?
        .ok_or(WasmError::Guest("Could not get game entry".into()))?;

    let game: GameEntry = element
        .entry()
        .to_app_option()?
        .ok_or(WasmError::Guest("Failed to convert game entry".into()))?;
    let signal = SignalPayload::Move(MoveInfo {
        move_hash: move_hash.clone().into(),
        move_entry: game_move,
    });

    signal::send_signal_to_players(game, signal)?;

    Ok(move_hash.into())
}

/**
 * Get all the moves for the given game
 */
pub fn get_game_moves(game_hash: EntryHashB64) -> ExternResult<Vec<MoveInfo>> {
    let moves = get_moves_entries(game_hash)?;

    Ok(moves
        .into_iter()
        .map(|(move_hash, move_entry)| MoveInfo {
            move_hash,
            move_entry,
        })
        .collect())
}

/**
 * Returns all the moves for the given game
 */
pub fn get_moves_entries(
    game_hash: EntryHashB64,
) -> ExternResult<Vec<(EntryHashB64, GameMoveEntry)>> {
    let links = get_links(EntryHash::from(game_hash), Some(game_to_move_tag()))?;

    let mut moves = links
        .into_inner()
        .into_iter()
        .map(|link| {
            let element = get(link.target.clone(), GetOptions::default())?
                .ok_or(WasmError::Guest("Couldn't get move".into()))?;
            let move_entry = element
                .entry()
                .to_app_option()?
                .ok_or(WasmError::Guest("Couldn't deserialize move".into()))?;

            Ok((link.target.into(), move_entry))
        })
        .collect::<ExternResult<Vec<(EntryHashB64, GameMoveEntry)>>>()?;

    order_moves(&mut moves)
}

/** Private helpers */

/**
 * Returns the moves ordered following the previous_move_address
 *
 * Returns error if in any case the chain of moves is not valid
 */
fn order_moves(
    moves: &mut Vec<(EntryHashB64, GameMoveEntry)>,
) -> ExternResult<Vec<(EntryHashB64, GameMoveEntry)>> {
    if moves.is_empty() {
        return Ok(vec![]);
    }

    // previous_move_hash -> next_move_hash
    let mut next_moves_map: HashMap<EntryHashB64, EntryHashB64> = HashMap::new();
    // move_hash -> move_entry
    let mut moves_map: HashMap<EntryHashB64, GameMoveEntry> = HashMap::new();

    let mut first_move: Option<EntryHashB64> = None;

    for move_entry in moves {
        if let Some(previous_move) = move_entry.1.previous_move_hash.clone() {
            if next_moves_map.contains_key(&previous_move) {
                return Err(WasmError::Guest(
                    "There are two moves pointing to the same next move".into(),
                ));
            }

            next_moves_map.insert(previous_move, move_entry.0.clone());
        } else {
            if let Some(_) = first_move {
                return Err(WasmError::Guest(
                    "There are two first moves in this list".into(),
                ));
            }
            first_move = Some(move_entry.0.clone());
        }

        if moves_map.contains_key(&move_entry.0) {
            return Err(WasmError::Guest(
                "There are two entries with the same hash in this list".into(),
            ));
        }

        moves_map.insert(move_entry.0.clone(), move_entry.1.clone());
    }

    match first_move {
        None => {
            return Err(WasmError::Guest(
                "There is no first move in this list".into(),
            ))
        }
        Some(first_move_hash) => {
            let mut ordered_moves: Vec<(EntryHashB64, GameMoveEntry)> = vec![];

            let mut maybe_next_move_hash: Option<EntryHashB64> = Some(first_move_hash);

            while let Some(next_move_hash) = maybe_next_move_hash {
                match moves_map.get(&next_move_hash) {
                    None => Err(WasmError::Guest(
                        "There are missing moves in the list".into(),
                    )),
                    Some(move_entry) => {
                        ordered_moves.push((next_move_hash.clone(), move_entry.clone()));
                        Ok(())
                    }
                }?;

                maybe_next_move_hash = next_moves_map.get(&next_move_hash).cloned();
            }

            Ok(ordered_moves)
        }
    }
}

fn game_to_move_tag() -> LinkTag {
    LinkTag::from(String::from("game->move").as_bytes().to_vec())
}
