# Deploy a new release

Add changes to the CHANGELOG.md file.
Build binaries and docker containers with `script/build.sh`.

Add a `github_token.inc` file to the root of the repository,
with a content like this:

```sh
GITHUB_TOKEN=an_access_token
GITHUB_USERNAME=your_username
```

Call `script/deploy.sh`.