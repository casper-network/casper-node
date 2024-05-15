# Highway State Analyzer

This tool analyzes a Highway protocol state dump and prints some information that may be helpful in explaining decreased reward amounts.

Usage: `highway-rewards-analysis [-v] FILE`

`FILE` should contain a Highway protocol state dump in the Bincode format, either plain or gzip-compressed. The `-v` flag causes the tool to print some additional information (see below).

The tool prints out 10 nodes that missed the most rounds in the era contained in the dump, as well as 10 nodes with the lowest average maximum level-1 summit quorum. The reward for a given block for a node is proportional to the maximum quorum of a level-1 summit containing that node in the round in which it was proposed - such quora for all the rounds in the era are what is averaged in the latter statistic.

If the `-v` flag is set, in addition to printing the 10 nodes with the most rounds missed, the tool also prints which rounds were missed by those nodes.
