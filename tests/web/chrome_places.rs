use bmo_explorer::web::chrome::places::*;
use bmo_explorer::web::clock::now_ms;

#[tokio::test]
async fn a_place_is_held_until_given_back_and_none_is_had_while_every_one_is_taken() {
  let dir = tempfile::tempdir().unwrap();
  let paths = vec![dir.path().join("one.lock"), dir.path().join("two.lock")];
  let first = take_any(&paths, now_ms()).await.unwrap();
  let second = take_any(&paths, now_ms()).await.unwrap();

  assert!(take_any(&paths, now_ms()).await.is_none());
  drop(first);
  // A process another test spawns in this moment holds a copy of the lock's descriptor until it execs, so the
  // place given back is waited for a little rather than asked for once.
  assert!(take_any(&paths, now_ms() + 5000).await.is_some());
  drop(second);
}
