export {
  BRIDGE_PORT,
  CLOSE_POLICY_VIOLATION,
  EXTENSION_ORIGIN_PREFIX,
  MAX_IMAGE_PIXELS,
  MAX_MESSAGE_BYTES,
  MAX_PNG_BYTES,
  PROTOCOL_VERSION,
} from './limits'
export {
  acceptedSchema,
  clientMessageSchema,
  encode,
  errorCodeSchema,
  errorMessageSchema,
  fullPageSchema,
  helloSchema,
  parseClientMessage,
  parseServerMessage,
  ProtocolError,
  readySchema,
  serverMessageSchema,
} from './messages'
export type {
  ClientMessage,
  ErrorCode,
  FullPage,
  Hello,
  ServerMessage,
} from './messages'
