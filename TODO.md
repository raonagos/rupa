# TODO List

### Initial Setup
- [x] initialize the project structure

### Core Features
- [x] implement CLI commands for `master` and `edge` modes
- [x] defines ENVironment variables needed
- [ ] design and set up a hexagonal architecture for database interaction
- [ ] use middleware to handle user authentication and login
- [ ] develop an API for managing WireGuard configurations
- [ ] create endpoints for managing user-specific WireGuard configurations

### Database Integration
- [ ] choose and integrate the database solution (eg, SurrealDB or another suitable option)
- [ ] implement database models for user management, configuration storage, etc
- [ ] prepare the database code to support future enhancements and scalability

### Additional Suggestions
- [ ] implement role-based access control for different types of users (eg, admin vs regular user)
- [ ] add logging and monitoring for server activities
- [ ] plan for multi-language support for the web client and CLI messages
- [ ] include basic error handling and fallback mechanisms for critical operations

### Future Preparations
- [ ] design the project structure to support additional VPN protocols or extensions
- [ ] outline future features such as user analytics, VPN usage reporting, or a dashboard
- [ ] prepare the codebase for integration with mobile and desktop clients
- [ ] ensure code modularity for easy updates and maintenance

### Documentation
- [ ] write comprehensive documentation for the CLI commands
- [ ] create developer guides for contributing to the project

### Testing
- [ ] set up a testing framework for the server and CLI commands
- [ ] write unit and integration tests for key functionalities
- [ ] prepare for end-to-end testing involving the web app and server
