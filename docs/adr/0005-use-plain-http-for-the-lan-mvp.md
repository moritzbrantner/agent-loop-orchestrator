# Use plain HTTP for the LAN MVP

The initial LAN dashboard serves plain HTTP and prints a newly generated bearer token at every startup, removing certificate and secret setup from the first-use path. This is intentionally limited to trusted networks: HTTP does not protect the token from network observers, so HTTPS must return before the service is exposed beyond that boundary.
