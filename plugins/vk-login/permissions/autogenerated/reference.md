## Default Permission

Intentionally empty. The sign-in is started from Rust (`VkLogin::start`), never
from the web UI: do not add `vk-login:allow-start` or `vk-login:allow-cancel`
to a capability, or page JavaScript could open the sign-in screen.

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`vk-login:allow-cancel`

</td>
<td>

Enables the cancel command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`vk-login:deny-cancel`

</td>
<td>

Denies the cancel command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`vk-login:allow-start`

</td>
<td>

Enables the start command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`vk-login:deny-start`

</td>
<td>

Denies the start command without any pre-configured scope.

</td>
</tr>
</table>
