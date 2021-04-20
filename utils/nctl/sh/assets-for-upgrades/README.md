In order to run a localised upgrade scenario:

1.  Set upgrade scenario root folder + specification file.

```
nctl-assets-for-upgrades-init-settings scenario=N
```

2.  Review settings.

```
vi $NCTL/assets-for-upgrades/scenario-N/settings.sh
```

3.  Initialise upgrade scenario staging assets.

```
nctl-assets-for-upgrades-init-staging scenario=N
```

4.  Review staged assets.

```
$NCTL/assets-for-upgrades/scenario-N/staging/X_Y_Z
$NCTL/assets-for-upgrades/scenario-N/staging/X_Y_Z/bin
$NCTL/assets-for-upgrades/scenario-N/staging/X_Y_Z/bin-client
$NCTL/assets-for-upgrades/scenario-N/staging/X_Y_Z/chainspec
$NCTL/assets-for-upgrades/scenario-N/staging/X_Y_Z/config
```

5.  Start network.

```
nctl-assets-for-upgrades-setup scenario=N
nctl-start
```

NOTE: this will inject activation point ...etc.

5.  Apply upgrade.

```
nctl-assets-for-upgrades-setup-next scenario=N semvar=X_Y_Z
```

NOTE: this will inject activation point ...etc.
