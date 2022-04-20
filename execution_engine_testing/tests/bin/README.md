# `disk_use` binary

A binary that will construct global state and profile the disk use of various operations. It's recommended to run this tool in `--release` mode.

It splits the results up into two CSV files:

- `disk_use_report.csv` - time-series data on the following columns, with one line per block:
- `height` - height of the simulated chain.
- `db-size` - size on disk of the backing trie database. 
- `transfers` - total number of transfers run.
- `time_ms` - time in milliseconds for a given block to run.
- `necessary_tries` - calculated value for the number of tries we expect to find in the trie database.
- `total_tries` - found total number of tries in the backing trie database.

Put together these two reports can be used to get a relatively quick view into disk and time cost of running transfers and auction processes.

