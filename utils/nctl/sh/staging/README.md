In order to run a localised staged scenarios:

1.  Set staging root folder + settings file.

```
nctl-stage-init-settings stage=${X:-1}
```

NOTE: stage identifer X is assumed to be an unsigned integer.

2.  Build staged assets.

```
nctl-stage-build-from-settings stage=${X:-1}
```

3.  Review staged assets.

```
$NCTL/stages/stage-N/X_Y_Z
$NCTL/stages/stage-N/X_Y_Z/bin
$NCTL/stages/stage-N/X_Y_Z/bin/wasm
$NCTL/stages/stage-N/X_Y_Z/resources
```

4.  Start network.

```
nctl-assets-setup-from-stage stage=${X:-1}
nctl-start
```

5.  Apply next upgrade.

```
nctl-assets-upgrade-from-stage stage=${X:-1} era=${Y:-$(($(get_next_era) + 2))}
```
