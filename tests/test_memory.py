from pathlib import Path

from damon.memory.store import JobStore


def test_job_store_survives_reopen(tmp_path: Path):
    db = tmp_path / "damon.db"
    store = JobStore(db)
    job = store.create("fix the tests")
    store.update(job.id, status="done", result="passed")
    reopened = JobStore(db).get(job.id)
    assert reopened is not None
    assert reopened.status == "done"
    assert reopened.result == "passed"
