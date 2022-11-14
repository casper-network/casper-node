#!/usr/bin/env bash
set -e

# DEFAULTS
PLUGIN_OS_CODENAME="${PLUGIN_OS_CODENAME:-bionic}"

# Verify all variables are present
if [[ -z $PLUGIN_GPG_KEY || -z $PLUGIN_GPG_PASS || -z $PLUGIN_REGION \
        || -z $PLUGIN_REPO_NAME || -z $PLUGIN_ACL || -z $PLUGIN_PREFIX \
        || -z $AWS_SECRET_ACCESS_KEY || -z $AWS_ACCESS_KEY_ID \
        || -z $PLUGIN_DEB_PATH || -z $PLUGIN_OS_CODENAME ]]; then
    echo "ERROR: Environment Variable Missing!"
    exit 1
fi

# Verify if its the first time publishing. Will need to know later.
# Probably an easier way to do this check :)
EXISTS=$(aws s3 ls s3://"$PLUGIN_REPO_NAME"/releases/dists/ --region "$PLUGIN_REGION" | grep "$PLUGIN_OS_CODENAME") || EXISTS_RET="false"

# Sanity Check for later
if [ "$EXISTS_RET" = "false" ]; then
    echo "First time uploading repo!"
else
    echo "Repo Exists! Defaulting to publish update..."
fi

### APTLY SECTION

# Move old config file to use in jq query
mv ~/.aptly.conf ~/.aptly.conf.orig

# Inject ENV Variables and save as .aptly.conf
jq --arg region "$PLUGIN_REGION" --arg bucket "$PLUGIN_REPO_NAME" --arg acl "$PLUGIN_ACL" --arg prefix "$PLUGIN_PREFIX"   '.S3PublishEndpoints[$bucket] = {"region":$region, "bucket":$bucket, "acl": $acl, "prefix": $prefix}' ~/.aptly.conf.orig > ~/.aptly.conf

# If aptly repo DOESNT exist locally already
if [ ! "$(aptly repo list | grep $PLUGIN_OS_CODENAME)" ]; then
    aptly repo create -distribution="$PLUGIN_OS_CODENAME" -component=main "release-$PLUGIN_OS_CODENAME"
fi

# If aptly mirror DOESNT exist locally already
if [ ! "$(aptly mirror list | grep $PLUGIN_OS_CODENAME)" ] && [ ! "$EXISTS_RET" = "false" ] ; then
    aptly mirror create -ignore-signatures "local-repo-$PLUGIN_OS_CODENAME" https://"${PLUGIN_REPO_NAME}"/"${PLUGIN_PREFIX}"/ "${PLUGIN_OS_CODENAME}" main
fi

# When it's not the first time uploading.
if [ ! "$EXISTS_RET" = "false" ]; then
    aptly mirror update -ignore-signatures "local-repo-$PLUGIN_OS_CODENAME"
    # Found an article that said using 'Name' will select all packages for us
    aptly repo import "local-repo-$PLUGIN_OS_CODENAME" "release-$PLUGIN_OS_CODENAME" Name
fi

# Add .debs to the local repo
aptly repo add -force-replace "release-$PLUGIN_OS_CODENAME" "$PLUGIN_DEB_PATH"/*.deb

# Publish to S3
if [ ! "$(aptly publish list | grep $PLUGIN_REPO_NAME | grep $PLUGIN_OS_CODENAME)" ]; then
    # If the repo is new
    aptly publish repo -batch -force-overwrite -passphrase="$PLUGIN_GPG_PASS" "release-$PLUGIN_OS_CODENAME" s3:"${PLUGIN_REPO_NAME}":
else
    # If the repo exists
    aptly publish update -batch -force-overwrite -passphrase="$PLUGIN_GPG_PASS" "$PLUGIN_OS_CODENAME" s3:"${PLUGIN_REPO_NAME}":
fi
