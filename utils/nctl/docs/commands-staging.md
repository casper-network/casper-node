# NCTL Staging Assets & Running Upgrades

## Overview

In order to run upgrade scenarios multiple assets sets are required, typically these are staged prior to scenario execution.  NCTL supports the initialisation of multiple stages - one per scenario.  Within the context of a stage NCTL supports multiple protocol version upgrades.


### nctl-stage-init-settings

Initialise stage settings.  This is done once per stage.

```
nctl-stage-init-settings stage=${X:-1}
```

NOTE: stage identifier X is assumed to be an unsigned integer.

### nctl-stage-build-from-settings

Build stage resources based upon declared settings.

```
nctl-stage-build-from-settings stage=${X:-1}
```

NOTE - when resources are staged it is wise to review staged them:

```
$NCTL/stages/stage-N/X_Y_Z
$NCTL/stages/stage-N/X_Y_Z/bin
$NCTL/stages/stage-N/X_Y_Z/bin/wasm
$NCTL/stages/stage-N/X_Y_Z/resources
```

### nctl-assets-setup-from-stage

Prepares assets from staged resources in readiness for network start.

```
nctl-assets-setup-from-stage stage=${X:-1}
nctl-start
```

### nctl-assets-upgrade-from-stage

Applies the next upgrade 2 eras from current.

```
nctl-assets-upgrade-from-stage stage=${X:-1} era=${Y:-$(($(get_next_era) + 2))}
```