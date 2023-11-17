#!/usr/bin/env python3

# pylint: disable=missing-module-docstring
# pylint: disable=line-too-long

import argparse
import logging
import pprint as pp
import sys
from pathlib import Path
from tomlkit import dumps
from tomlkit import loads


logger = logging.getLogger(__name__)


def main():
    """The meat and potatoes."""
    args = parse_args()
    logger = logger_setup(args.log_level)
    logger.info("Input args: %s", vars(args))

    logger.info("Opening overrides toml file: %s", args.override_file)
    override_dict = load_toml_file(args.override_file)

    logger.info("Opening toml file: %s", args.toml_file)
    toml_dict = load_toml_file(args.toml_file)

    logger.info("Applying overrides to toml file")
    new_toml_dict = edit_toml(override_dict, toml_dict, args.no_skip)

    if args.output_file is not None:
        logger.info("Generating toml file to: %s", args.output_file)
        write_toml(args.output_file, new_toml_dict)
    else:
        pp.pprint(new_toml_dict)

    logger.info("Finished!")


def logger_setup(level):
    """Sets up logger based on based in log level."""
    level = level.lower()

    if level == "debug":
        log_format = (
            "[%(asctime)s] %(levelname)s [%(name)s.%(funcName)s:%(lineno)d] %(message)s"
        )
    else:
        log_format = "[%(asctime)s] %(levelname)s %(message)s"

    levels = {"info": logging.INFO, "debug": logging.DEBUG, "warning": logging.WARNING}
    level = levels.get(level)

    logging.basicConfig(
        stream=sys.stdout, level=level, format=log_format, datefmt="%Y-%m-%d %H:%M:%S"
    )
    return logging.getLogger(__name__)


def parse_args():
    """Sets up arguments from CLI input."""
    parser = argparse.ArgumentParser(
        description="A script to generate casper-node configs with overridden values"
    )
    parser.add_argument("-f", "--toml_file", help="Path to toml file", required=True)
    parser.add_argument(
        "-o", "--override_file", help="Path to override file", required=True
    )
    parser.add_argument(
        "-l",
        "--log_level",
        help="Sets log level. (default: info)",
        required=False,
        default="info",
        choices=["info", "debug", "warning"],
    )
    parser.add_argument(
        "--output_file",
        help="Path to write new toml to. If not specified, file is written to stdout",
        required=False,
        default=None,
    )
    parser.add_argument(
        "--no_skip",
        help="Ignores default behavior. Will not skip keys not found in original toml file.",
        required=False,
        action='store_true',
    )
    args = parser.parse_args()
    return args


def write_toml(file_path, toml_dict):
    """Writes TOMLDocument to file path."""
    logger.debug(pp.pformat(toml_dict))
    logger.debug("file path: %s", file_path)
    Path(file_path).write_text(dumps(toml_dict))


def edit_toml(override_dict, toml_dict, no_skip):
    """Returns updated toml dictionary."""
    logger.debug(pp.pformat(override_dict))
    logger.debug(pp.pformat(toml_dict))
    new_dict = dict(mergedicts(toml_dict, override_dict, no_skip))
    return new_dict


def mergedicts(dict1, dict2, no_skip=False):
    """Merges two dictionaries where applicable"""
    for k in set(dict1) | set(dict2):
        if k in dict1 and k in dict2:
            if isinstance(dict1[k], dict) and isinstance(dict2[k], dict):
                yield k, dict(mergedicts(dict1[k], dict2[k], no_skip))
            else:
                # value from dict2 overrides one in dict1.
                yield k, dict2[k]
        elif k in dict1:
            # not in dict2, keep values of dict1
            yield k, dict1[k]
        else:
            if no_skip:
                # non-default behavior, allow unmatched keys
                logger.warning("no_skip passed: allowing %s", k)
                yield k, dict2[k]
            else:
                # do nothing if its not present in dict1
                logger.warning("Skipping %s: not found in original toml", k)


def load_toml_file(file_path):
    """returns toml file as TOMLDocument."""
    toml_dict = loads(Path(file_path).read_text())
    logger.debug(pp.pformat(toml_dict))
    return toml_dict


if __name__ == "__main__":
    main()
