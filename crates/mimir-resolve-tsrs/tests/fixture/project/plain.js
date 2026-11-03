function first() {}
function second() {
  first();
}
class Old {
  run() {
    second();
  }
}
module.exports = { Old };
