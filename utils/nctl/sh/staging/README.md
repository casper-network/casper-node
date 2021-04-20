In order to run a localised staged scenarios:

1.  Set staging root folder + settings file.

```
nctl-staging-init
```

2.  Initialise staged assets.

```
nctl-staging-stage
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
nctl-staging-setup
nctl-start
```

NOTE: this will inject activation point ...etc.

5.  Apply upgrade.

```
nctl-staging-setup-next
```

NOTE: this will inject activation point ...etc.
