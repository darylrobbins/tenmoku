# T690: a_running_build_grows_its_bars flake

**Cause.** The test's last step asserted `rows[1].name == "app"`. Rows are
ordered by span start, and the fresh `itoa` crate and `app` begin within the
same moments, so the order depended on timing (seen locally: left `itoa`,
right `app`). The code under test is correct; only the test assumed an order.

**Fix.** Test only: look the `app` row up by name.

**Evidence.** Before: 1 failure in 6 sequential runs at the assertion above.
After: 20 consecutive runs of the test binary
(`fe_workbench tests::build_timeline_view::a_running_build_grows_its_bars`)
while three loops ran the whole `tests::` suite concurrently: 20 passed, 0
failed.
