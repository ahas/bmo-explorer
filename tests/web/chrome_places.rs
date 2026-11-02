use bmo_explorer::web::chrome::places::*;

#[tokio::test]
async fn a_place_is_held_until_given_back_and_a_start_waits_for_one_while_every_one_is_taken() {
  let dir = tempfile::tempdir().unwrap();
  let paths = vec![dir.path().join("one.lock"), dir.path().join("two.lock")];
  let first = take_any(&paths).await.unwrap();
  let second = take_any(&paths).await.unwrap();
  let mut waiting = Box::pin(take_any(&paths));

  // Every place is taken: the third start waits rather than being turned away.
  assert!(futures_util::poll!(waiting.as_mut()).is_pending());
  drop(first);
  // A process another test spawns in this moment holds a copy of the lock's descriptor until it execs, so the
  // place given back may come a moment later; the start waits for it either way.
  assert!(waiting.await.is_ok());
  drop(second);
}

#[tokio::test]
async fn a_place_whose_lock_file_cannot_be_opened_fails_the_start() {
  let dir = tempfile::tempdir().unwrap();
  let paths = vec![dir.path().join("missing").join("one.lock")];

  assert!(take_any(&paths).await.is_err());
}
