/**
 * A move the listener already made for itself, after finding its remembered
 * port taken.
 *
 * The port is diagnostic and appears nowhere else in the interface; the user is
 * never asked to choose one.
 */
export interface PortMove {
  from: number
  to: number
}
