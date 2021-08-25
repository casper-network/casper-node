#!/usr/bin/env python3
from github import Github
from sys import exit
import os
import semantic_version

# Environment Variables
token = os.getenv('GITHUB_TOKEN')
tag = os.getenv('DRONE_TAG')
drone_repo = os.getenv('DRONE_REPO')

# Remove the 'v' from our tag
version = tag[1:]

# Concat release- + tagged version
# NOTE: Went this route because branches aren't associated
#       with tags.
change_to_branch = 'release-' + version

# Validate tag is semver compliant
if not semantic_version.validate(version):
    print(f'ERROR: Could not validate semantic version: {version}!')
    print("... Please check the tag used! Exiting.")
    exit(1)

# Login with token from environment variable
g = Github(token)
# Working repo
repo = g.get_repo(drone_repo)
# Working branch
branch = repo.get_branch(branch=change_to_branch)

print(f'Branch {change_to_branch} found! Continuing...')

# Get the default branch of repo
default_branch = repo.default_branch

# Validate tag is semver compliant
if "default_branch" in globals() :
    print(f'... Found current default branch: {default_branch}')
    print(f'... Switching default branch to: {change_to_branch}')
    # Switch default branch
    repo.edit(default_branch=change_to_branch)
else:
    print('ERROR: Default branch was not found!')
    exit(1)

# Get new default branch after change
new_def_branch = repo.default_branch

# Validate branch change was successful
if new_def_branch == change_to_branch:
    print(f'... Success! New default branch: {new_def_branch}')
    print(f'... Setting branch restrictions: {new_def_branch}')
    # Create branch restriction
    branch.edit_protection(enforce_admins=True,
                            required_approving_review_count=1,
                            strict=False,
                            contexts=["continuous-integration/drone/pr","continuous-integration/drone/push"],
                            user_push_restrictions=["casperlabs-drone"])
    print('... Success!')
else:
    print('ERROR: Branch change failure')
    print(f'... Found default branch: {new_def_branch}')
    print(f'... Expected default branch: {change_to_branch}')
    exit(1)
