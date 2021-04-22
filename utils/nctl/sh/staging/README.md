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
$NCTL/staging/X_Y_Z
$NCTL/staging/X_Y_Z/bin
$NCTL/staging/X_Y_Z/bin-client
$NCTL/staging/X_Y_Z/chainspec
$NCTL/staging/X_Y_Z/config
```

4.  Start network.

```
nctl-assets-setup-from-stage stage=${X:-1}
nctl-start
```

5.  Apply next upgrade.

```
nctl-assets-upgrade-from-stage stage=${X:-1} era=${Y:-$(($(get_next_era) + 1))}
```
