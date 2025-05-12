#![no_std]
#![allow(static_mut_refs)]
use gstd::exec::*;
use gstd::{debug, exec, msg, prelude::*};
use wordle_game_session_io::*;

const COUNT_WORD: usize = 5;

static mut WORDLE_STATE: Option<WordleState> = None;

fn get_wordle_state() -> &'static mut WordleState {
    unsafe {
        let game_state = WORDLE_STATE
            .as_mut()
            .expect("WORDLE_STATE isn't initialized");
        game_state
    }
}

#[no_mangle]
extern "C" fn init() {
    let wordle_init = msg::load::<WordleInit>().expect("Failed to load");
    unsafe {
        WORDLE_STATE = Some(WordleState {
            wordle_address: wordle_init.wordle_address,
            status: WordleStatus::Init,
            count_attemps: wordle_init.count_attempts,
            delay_timeout: wordle_init.delay_timeout,
        })
    }
}

#[no_mangle]
unsafe extern "C" fn handle() {
    let user_id = msg::source();
    let msg_id = msg::id();
    let state = get_wordle_state();
    let action: WordleAction = msg::load().expect("Failed to load payload");

    debug!("=== [handle] ===");
    debug!("current status: {:?}", state.status);
    debug!("action: {:?}", action);

    if action == WordleAction::CheckGameStatus {
        let status = match &state.status {
            WordleStatus::GameOver(status) => status.clone(),
            _ => WordlePlayerStatus::Loose,
        };
        debug!("CheckGameStatus, reply: {:?}", status);
        msg::reply(
            match status {
                WordlePlayerStatus::Win => WordleEvent::YouAreWin,
                WordlePlayerStatus::Loose => WordleEvent::YouAreLoose,
            },
            0,
        )
        .expect("Failed to reply");
        return;
    }

    match &state.status {
        WordleStatus::Init => {
            debug!("status: Init");
            if action == WordleAction::StartGame {
                debug!("StartGame action");
                msg::send_delayed(
                    exec::program_id(),
                    WordleAction::CheckGameStatus,
                    0,
                    state.delay_timeout,
                )
                .expect("Failed to send");

                let sent_id = msg::send(
                    state.wordle_address,
                    wordle_game_io::Action::StartGame { user: user_id },
                    0,
                )
                .expect("Failed to send");

                state.status = WordleStatus::GameStartMessageSent {
                    orig_id: msg_id,
                    sent_id,
                };

                debug!("set status: GameStartMessageSent");
                wait();
            } else {
                debug!("WrongActionToTrigger in Init");
                msg::reply(WordleEvent::WrongActionToTrigger(state.clone(), action), 0)
                    .expect("Failed to reply");
            }
        }
        WordleStatus::GameStartMessageSent { .. } => {
            debug!("WrongActionToTrigger in GameStartMessageSent");
            msg::reply(WordleEvent::WrongActionToTrigger(state.clone(), action), 0)
                .expect("Failed to reply");
        }
        WordleStatus::GameStartMessageReceived { event } => match event {
            wordle_game_io::Event::GameStarted { user: _ } => {
                debug!("GameStartMessageReceived: GameStarted");
                state.status = WordleStatus::GameStarted;
                msg::reply(WordleEvent::GameStartSuccess, 0).expect("Failed to reply");
            }
            _ => {
                debug!("GameStartMessageReceived: GameStartFail");
                msg::reply(WordleEvent::GameStartFail(event.clone()), 0).expect("Failed to reply");
            }
        },
        WordleStatus::CheckWordMessageSent { .. } => {
            debug!("WrongActionToTrigger in CheckWordMessageSent");
            msg::reply(WordleEvent::WrongActionToTrigger(state.clone(), action), 0)
                .expect("Failed to reply");
        }
        WordleStatus::CheckWordMessageReceived { event } => {
            debug!("CheckWordMessageReceived, event: {:?}", event);
            let send_event = event.clone();
            match event {
                wordle_game_io::Event::WordChecked {
                    user: _,
                    ref correct_positions,
                    ref contained_in_word,
                } => {
                    debug!("WordChecked: correct_positions = {:?}, contained_in_word = {:?}, count_attemps = {}", correct_positions, contained_in_word, state.count_attemps);
                    if correct_positions.len() == COUNT_WORD && contained_in_word.is_empty() {
                        debug!("GameOver: Win");
                        state.status = WordleStatus::GameOver(WordlePlayerStatus::Win);
                        msg::reply(WordleEvent::CheckWordSuccess(send_event), 0)
                            .expect("Failed to reply");
                    } else {
                        state.count_attemps -= 1;
                        debug!("count_attemps after -=1: {}", state.count_attemps);
                        if state.count_attemps == 0 {
                            debug!("GameOver: Loose");
                            state.status = WordleStatus::GameOver(WordlePlayerStatus::Loose);
                            msg::reply(WordleEvent::CheckWordSuccess(send_event), 0)
                                .expect("Failed to reply");
                        } else {
                            state.status = WordleStatus::GameStarted;
                            msg::reply(WordleEvent::CheckWordSuccess(send_event), 0)
                                .expect("Failed to reply");
                        }
                    }
                }
                _ => {
                    debug!("CheckWordFail");
                    msg::reply(WordleEvent::CheckWordFail(send_event), 0).expect("Failed to reply");
                }
            }
        }
        WordleStatus::GameStarted => {
            debug!("status: GameStarted");
            if let WordleAction::CheckWord(word) = action {
                debug!("CheckWord action, word: {:?}", word);
                let sent_id = msg::send(
                    state.wordle_address,
                    wordle_game_io::Action::CheckWord {
                        user: user_id,
                        word,
                    },
                    0,
                )
                .expect("Failed to send");

                state.status = WordleStatus::CheckWordMessageSent {
                    orig_id: msg_id,
                    sent_id,
                };
                debug!("set status: CheckWordMessageSent");
                wait();
            } else {
                debug!("WrongActionToTrigger in GameStarted");
                msg::reply(WordleEvent::WrongActionToTrigger(state.clone(), action), 0)
                    .expect("Failed to reply");
            }
        }
        WordleStatus::GameOver(status) => {
            debug!("status: GameOver({:?})", status);
            let status = match status {
                WordlePlayerStatus::Win => WordleEvent::YouAreWin,
                WordlePlayerStatus::Loose => WordleEvent::YouAreLoose,
            };
            msg::reply(status, 0).expect("Failed to reply");
        }
    };
}

#[no_mangle]
unsafe extern "C" fn handle_reply() {
    let reply_to = msg::reply_to().expect("Failed to get reply_to");
    let event: wordle_game_io::Event = msg::load().expect("Failed to load payload");
    let state = get_wordle_state();

    debug!("=== [handle_reply] ===");
    debug!("reply_to: {:?}", reply_to);
    debug!("event: {:?}", event);
    debug!("current status: {:?}", state.status);

    match state.status {
        WordleStatus::GameStartMessageSent { orig_id, sent_id } if reply_to == sent_id => {
            debug!("GameStartMessageSent -> GameStartMessageReceived");
            state.status = WordleStatus::GameStartMessageReceived { event };
            wake(orig_id).expect("Failed to wake message");
        }
        WordleStatus::CheckWordMessageSent { orig_id, sent_id } if reply_to == sent_id => {
            debug!("CheckWordMessageSent -> CheckWordMessageReceived");
            state.status = WordleStatus::CheckWordMessageReceived { event };
            wake(orig_id).expect("Failed to wake message");
        }
        _ => {
            debug!("Unexpected state in handle_reply: {:?}", state.status);
            todo!();
        }
    };
}

#[no_mangle]
unsafe extern "C" fn state() {
    let state = get_wordle_state();
    msg::reply(state, 0).expect("Unable to share the state");
}
