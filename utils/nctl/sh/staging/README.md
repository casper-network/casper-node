In order to run a localised staged scenarios:

1.  Set staging root folder + settings file.

```
nctl-stage-init-settings stage=N
```

2.  Build staged assets.

```
nctl-stage-build-from-settings stage=N
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
nctl-assets-setup-from-stage stage=N
nctl-start
```

5.  Apply next upgrade.

```
nctl-assets-upgrade-from-stage stage=N
```
