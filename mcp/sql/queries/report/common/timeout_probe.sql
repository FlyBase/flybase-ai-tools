-- Test-only: sleeps past the light statement timeout so a per-section error can be observed (see report::timeout_probe).
select pg_sleep(12)
