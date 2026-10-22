## Default Permission

Lets the webview ask whether background playback is set up (notification
permission, battery optimisation) and open the system battery settings screen.
The playback commands themselves are Rust-only and need no permission.

#### This default permission set includes the following:

- `allow-status`
- `allow-open-battery-settings`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`media-session:allow-open-battery-settings`

</td>
<td>

Enables the open_battery_settings command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`media-session:deny-open-battery-settings`

</td>
<td>

Denies the open_battery_settings command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`media-session:allow-status`

</td>
<td>

Enables the status command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`media-session:deny-status`

</td>
<td>

Denies the status command without any pre-configured scope.

</td>
</tr>
</table>
