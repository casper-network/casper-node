#!/usr/bin/bash
for f in ./src/*.c; do
    OUTPUT=$(diff <(clang-format $f) <(cat $f));
    ret=$?
    if [[ $ret -ne 0 ]]; then
        echo "Source file format $f differs - try running:"
        echo ""
        echo "clang-format -i $f"
        echo ""
        echo $"$OUTPUT"
        exit $ret
    fi
done
