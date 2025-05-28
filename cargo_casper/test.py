#!/usr/bin/env python
import itertools
import time
import sys
import os

for a in itertools.count():
    if a % 2 == 0:
        print(f'line {a}')
    else:
        print(f'error line {a}', file=sys.stderr)
    time.sleep(0.05)
    if a == 44:
        os.kill(os.getpid(), 9)
    if a > 100:
        break

print('Goodbye')
