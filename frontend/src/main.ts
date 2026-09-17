import { connect } from "./ws-client";
import { renderState, setConnectionStatus } from "./render";

connect({
  onMessage: renderState,
  onConnectionChange: setConnectionStatus,
});
