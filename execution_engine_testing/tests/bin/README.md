# `disk_use` binary

A binary that will construct global state and profile the disk use of various operations.

It splits the results up into two CSV files:

- `bytes-report-{}.csv` - time-series data over bytes on disk vs number of transfers
- `time-report-{}.csv` - time-series data over time spent vs number of transfers

Put together these two reports can be used to get a relatively quick view into disk and time cost of running transfers and auction processes.
