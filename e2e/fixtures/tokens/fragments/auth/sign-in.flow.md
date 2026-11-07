# Sign in

The login every case-handler session starts with.

## When

1. **Sign in with a username and password**
   ```browser
   - goto: /login
   - fill: { label: "Username or email", value: "{{username}}" }
   - fill: { label: Password, value: "{{password}}" }
   - click: Sign In
   ```

## Then

2. **The dashboard is shown**
   ```expect
   heading: Dashboard
   ```
