# Protect LAN access with a shared token

The service may be exposed to the local network because the owner explicitly needs LAN access, but every API request must carry a bearer token generated and displayed when the service starts. The service can launch coding agents with access to local repositories, so an unauthenticated LAN endpoint is not an acceptable boundary; a single short-lived token is sufficient for this personal-service slice and avoids adding account management or secret setup.
